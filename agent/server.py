#!/usr/bin/env python3
"""Local chat server using litert_lm with real token streaming."""

from __future__ import annotations

import asyncio
import json
import logging
import os
import queue
import tempfile
import threading
from contextlib import asynccontextmanager
from datetime import datetime
from pathlib import Path
from typing import Any, AsyncGenerator

import litert_lm
import uvicorn
from fastapi import FastAPI, File, Form, UploadFile
from fastapi.responses import HTMLResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles

# from tool import get_weather, web_browser, web_fetch, web_search
from tool import get_weather, web_fetch, web_search

ROOT = Path(__file__).resolve().parent
DEFAULT_MODEL = "gemma-4-E2B-it.litertlm"

BASE_SYSTEM_PROMPT = "You are a multimodal model. You can directly process and understand input from images and audio."

DEFAULT_SYSTEM_PROMPT = (
    "IDENTITY: You are Buddy, an advanced Large Language Model. "
    "STYLE: Be concise in all responses. Prioritize directness and accuracy."
    "CORE DIRECTIVES (CRITICAL):"
    "1. Factuality: Do not guess, speculate, or fill in gaps in information. If you do not know the answer, state clearly that you do not know."
    "2. Tool Use: If you decide to use a tool, respond *only* with the information retrieved from the tool. Do not add commentary or elaboration."
    "3. Grounding: After using any tool, your final response must be based *only* on the information returned by that tool."
    #
    # "You are a research coordinator. When asked to check multiple sites:\n"
    # "1. Call web_fetch for each URL in parallel\n"
    # "2. Synthesize results\n"
    # "3. Provide comprehensive answer\n"
)

logging.basicConfig(level=logging.INFO)
log = logging.getLogger("buddy")


# ── helpers


def save_temp_file(data: bytes, suffix: str) -> str:
    tmp = tempfile.NamedTemporaryFile(suffix=suffix, delete=False)
    tmp.write(data)
    tmp.close()
    return tmp.name


def cleanup_files(*paths: str | None) -> None:
    for p in paths:
        if p:
            try:
                os.unlink(p)
            except OSError:
                pass


engine = None


def load_dotenv() -> None:
    env_path = ROOT / ".env"
    if not env_path.exists():
        return
    for raw_line in env_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if value[:1] == value[-1:] and value[:1] in {'"', "'"}:
            value = value[1:-1]
        os.environ.setdefault(key, value)


def resolve_model_path() -> str:
    path = os.environ.get("MODEL_PATH", "")
    if path:
        return path
    local_model = ROOT / DEFAULT_MODEL
    if local_model.exists():
        return str(local_model)
    from huggingface_hub import hf_hub_download

    repo = os.environ.get("HF_REPO", "litert-community/gemma-4-E2B-it-litert-lm")
    filename = os.environ.get("HF_FILENAME", DEFAULT_MODEL)
    print(f"Downloading {repo}/{filename} (first run only)...")
    return hf_hub_download(repo_id=repo, filename=filename)


def init_engine() -> None:
    global engine
    model_path = resolve_model_path()
    print(f"Loading model from {model_path}...")
    backend_str = os.environ.get("BACKEND", "GPU").upper()
    vision_str = os.environ.get("VISION_BACKEND", "GPU").upper()
    audio_str = os.environ.get("AUDIO_BACKEND", "CPU").upper()
    backend = getattr(litert_lm.Backend, backend_str, litert_lm.Backend.GPU)
    vision_backend = getattr(litert_lm.Backend, vision_str, litert_lm.Backend.GPU)
    audio_backend = getattr(litert_lm.Backend, audio_str, litert_lm.Backend.CPU)
    engine = litert_lm.Engine(
        model_path,
        backend=backend,
        vision_backend=vision_backend,
        audio_backend=audio_backend,
    )
    engine.__enter__()
    print("Engine loaded successfully.")


def cleanup_engine() -> None:
    global engine
    if engine:
        engine.__exit__(None, None, None)
        engine = None


def get_current_time() -> str:
    return datetime.now().astimezone().strftime("%A, %B %d, %Y, %I:%M:%S %p %Z")


def get_system_prompt() -> str:
    base_prompt = os.environ.get("LLM_SYSTEM_PROMPT", DEFAULT_SYSTEM_PROMPT)
    return f"{BASE_SYSTEM_PROMPT}\n{base_prompt}\nCurrent date and time: {get_current_time()}"


def normalize_messages(messages: list[dict[str, Any]]) -> list[dict[str, str]]:
    normalized: list[dict[str, str]] = []
    for message in messages:
        role = message.get("role", "user")
        content = message.get("content")
        if isinstance(content, str):
            text = content
        else:
            parts = message.get("parts", [])
            if isinstance(content, list):
                parts = content
            text_chunks = []
            for part in parts:
                if isinstance(part, dict):
                    if part.get("type") == "text":
                        text_chunks.append(part.get("text", ""))
                    elif "text" in part:
                        text_chunks.append(part["text"])
            text = "".join(text_chunks)
        normalized.append({"role": role, "content": text})
    return normalized


# ── real async streaming generator


async def generate_stream_async(
    messages: list[dict[str, Any]],
    last_message: dict[str, Any],
) -> AsyncGenerator[str, None]:
    """
    Uses send_message_async to yield real tokens as they are generated.
    Runs the litert_lm synchronous iterator in a background thread,
    passing tokens to FastAPI via a thread-safe queue.
    """
    if engine is None:
        raise RuntimeError("Engine not initialized")

    normalized = normalize_messages(messages)
    full_messages: list[dict[str, Any]] = [
        {"role": "system", "content": get_system_prompt()}
    ]
    full_messages.extend(normalized)

    q: queue.Queue[str | None] = queue.Queue()

    class _Handler(litert_lm.ToolEventHandler):
        def approve_tool_call(self, tool_call: dict[str, Any]) -> bool:
            fn = tool_call.get("function", {}).get("name", "tool")
            log.info("Tool call approved: %s", fn)
            q.put(json.dumps({"type": "tool_use", "name": fn}) + "\n")
            return True

        def process_tool_response(
            self, tool_response: dict[str, Any]
        ) -> dict[str, Any]:
            log.info("Tool response: %s", str(tool_response)[:200])
            return tool_response

    handler = _Handler()

    def run_in_thread() -> None:
        try:
            if engine is None:
                q.put(
                    json.dumps({"type": "error", "text": "Engine not initialized"})
                    + "\n"
                )
                q.put(None)
                return

            with engine.create_conversation(
                messages=full_messages,
                tools=[
                    # web_browser,
                    get_weather,
                    web_search,
                    web_fetch,
                ],
                tool_event_handler=handler,
            ) as conv:
                # MessageIterator is a synchronous iterator — use plain `for`
                for chunk in conv.send_message_async(last_message):
                    text = ""
                    if isinstance(chunk, str):
                        text = chunk
                    elif isinstance(chunk, dict):
                        for item in chunk.get("content", []):
                            if isinstance(item, dict) and item.get("type") == "text":
                                text += item.get("text", "")
                        if not text:
                            text = chunk.get("text", "")

                    if text:
                        q.put(json.dumps({"type": "text", "text": text}) + "\n")

        except Exception as e:
            log.exception("Error in streaming thread")
            q.put(json.dumps({"type": "error", "text": str(e)}) + "\n")
        finally:
            q.put(None)  # Sentinel to signal end of stream

    # Start the background thread
    t = threading.Thread(target=run_in_thread, daemon=True)
    t.start()

    # Pull items from the thread-safe queue asynchronously
    while True:
        item = await asyncio.get_running_loop().run_in_executor(None, q.get)
        if item is None:
            break
        yield item

    t.join(timeout=5)


# ── FastAPI app


@asynccontextmanager
async def lifespan(app: FastAPI):
    load_dotenv()
    try:
        init_engine()
        yield
    finally:
        cleanup_engine()


app = FastAPI(lifespan=lifespan)
app.mount("/static", StaticFiles(directory="static"), name="static")


@app.get("/")
async def root():
    return HTMLResponse(content=(ROOT / "index.html").read_text())


@app.get("/api/health")
async def health():
    return {"status": "ok", "engine_loaded": engine is not None}


@app.post("/api/chat")
async def chat(
    messages: str = Form(...),
    audio: UploadFile | None = File(None),
    image: UploadFile | None = File(None),
):
    messages_list: list[dict[str, Any]] = json.loads(messages)
    audio_path: str | None = None
    image_path: str | None = None

    if audio:
        audio_path = save_temp_file(await audio.read(), ".wav")
    if image:
        image_path = save_temp_file(await image.read(), ".jpg")

    history = messages_list[:-1] if messages_list else []

    # Extract user's actual text message
    last = messages_list[-1] if messages_list else {"role": "user", "content": ""}
    user_text = ""
    if isinstance(last.get("content"), str):
        user_text = last["content"]
    elif isinstance(last.get("content"), list):
        user_text = " ".join(
            p.get("text", "")
            for p in last["content"]
            if isinstance(p, dict) and p.get("type") == "text"
        )

    if audio_path or image_path:
        content: Any = []
        if audio_path:
            content.append({"type": "audio", "path": os.path.abspath(audio_path)})
        if image_path:
            content.append({"type": "image", "path": os.path.abspath(image_path)})
        # Include user's actual text with media context
        if audio_path and image_path:
            content.append(
                {
                    "type": "text",
                    "text": user_text
                    or "Respond to what they said and describe what you see.",
                }
            )
        elif audio_path:
            content.append(
                {"type": "text", "text": user_text or "Respond to what they said."}
            )
        else:
            content.append(
                {"type": "text", "text": user_text or "Describe what you see."}
            )
    else:
        content = user_text or "Hello"

    last_message = {"role": "user", "content": content}

    async def stream() -> AsyncGenerator[bytes, None]:
        try:
            async for chunk in generate_stream_async(history, last_message):
                yield chunk.encode("utf-8")
        finally:
            cleanup_files(audio_path, image_path)

    return StreamingResponse(
        stream(),
        media_type="application/x-ndjson",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
        },
    )


def main():
    port = int(os.environ.get("PORT", "3000"))
    host = os.environ.get("HOST", "0.0.0.0")
    ssl_keyfile = os.environ.get("SSL_KEYFILE")
    ssl_certfile = os.environ.get("SSL_CERTFILE")
    print(f"Starting server on {host}:{port}")
    if ssl_keyfile and ssl_certfile:
        print(f"SSL enabled: {ssl_certfile}")
    uvicorn.run(
        app, host=host, port=port, ssl_keyfile=ssl_keyfile, ssl_certfile=ssl_certfile
    )


if __name__ == "__main__":
    load_dotenv()
    main()
