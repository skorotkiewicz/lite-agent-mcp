# LLM Helper Server

A minimal MCP (Model Context Protocol) server in Rust that helps LLMs browse webpages and search the web. Includes a Python agent wrapper for local models that can't do multi-step reasoning.

## Features

- **MCP Protocol**: Full MCP 2025-03-26 support via HTTP POST
- **Web Browsing**: Fetch and extract content from any webpage
- **Web Search**: Search the web using DuckDuckGo
- **Link Extraction**: Extract all links from a webpage
- **Agent Wrapper**: Multi-step exploration for local 4B models that can't plan tool chains

## Architecture

```
┌─────────────┐     HTTP      ┌─────────────────┐
│  LLM Agent  │◄─────────────►│  llm-helper     │
│  (Python)   │  MCP Protocol │  (Rust/Axum)    │
│  E4B model  │               │  Port 3000      │
└─────────────┘               └────────┬────────┘
                                         │
                                ┌────────┴────────┐
                                │  Web Scraping   │
                                │  (reqwest+html2text)
                                └─────────────────┘
```

**Why two layers?** Local small models (4B) can't plan multi-step tool calls (browse → not found → extract links → browse subpage). The agent wrapper handles the exploration loop in Python, making one tool call that does all the work.

## Building

```bash
cargo build --release
```

## Running

### HTTP Mode (Default)

```bash
./target/release/llm-helper --port 3000
```

The MCP endpoint will be available at `http://127.0.0.1:3000/mcp`

### MCP Stdio Mode (for Claude Desktop)

```bash
./target/release/llm-helper --mcp-stdio
```

## MCP Endpoint

Only one endpoint: `POST /mcp`

### Initialize

```bash
curl -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
      "protocolVersion": "2025-03-26",
      "capabilities": {},
      "clientInfo": {"name": "test", "version": "1.0.0"}
    }
  }'
```

Response includes session ID in `mcp-session-id` header.

### Call Tool

```bash
curl -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -H "mcp-session-id: <session-id>" \
  -d '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {
      "name": "browse_web",
      "arguments": {
        "url": "https://example.com",
        "extract_text": true
      }
    }
  }'
```

## MCP Tools

### Core Tools (Rust Server)

1. **browse_web** - Fetch and extract content from a webpage
   - Parameters: `url` (string), `extract_text` (boolean, optional)
   - Returns: Title, text content, links count, images count

2. **search_web** - Search the web using DuckDuckGo
   - Parameters: `query` (string), `limit` (integer, optional)
   - Returns: Search results with titles, URLs, and snippets

3. **extract_links** - Extract all links from a webpage
   - Parameters: `url` (string)
   - Returns: List of all HTTP/HTTPS links found

### Agent Tools (Python Wrapper)

For local models that can't plan multi-step exploration:

4. **agent_find_phone** - Find phone number on a website (auto-explores subpages)
   - Parameters: `url` (string)
   - Returns: Phone number or "not found"

5. **agent_find_email** - Find email on a website (auto-explores subpages)
   - Parameters: `url` (string)
   - Returns: Email address or "not found"

## Usage Examples

### Direct MCP (Cloud LLM / Advanced Models)

Models that can plan tool chains can use the core tools directly:

```
browse_web("https://sekor.eu.org") 
→ "Posts Notes Tags Contact..."

extract_links("https://sekor.eu.org")
→ "https://sekor.eu.org/contact/"

browse_web("https://sekor.eu.org/contact/")
→ "Phone: +49 (0)159 0268 1236"
```

### Via Agent Wrapper (Local 4B Models)

Models that struggle with multi-step reasoning use the agent tools:

```
agent_find_phone("https://sekor.eu.org")
→ "Found phone: +49 (0)159 0268 1236 (Source: https://sekor.eu.org/contact/)"

agent_find_email("https://sekor.eu.org")
→ "Found email: skorotkiewicz@gmail.com (Source: https://sekor.eu.org/contact/)"
```

The agent automatically:
1. Browses the initial page
2. Looks for the target (phone/email)
3. If not found, extracts all links
4. Filters promising links (`/contact`, `/about`, etc.)
5. Browses each until found or exhausted

## MCP Client Configuration

### Native Tool (OpenCode, etc.)

```json
{
  "llm-helper": {
    "type": "remote",
    "url": "http://127.0.0.1:3000/mcp",
    "enabled": true
  }
}
```

### Stdio Mode (Claude Desktop, etc.)

**macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "llm-helper": {
      "command": "/path/to/llm-helper",
      "args": ["--mcp-stdio"]
    }
  }
}
```

### HTTP Mode

```json
{
  "mcpServers": {
    "llm-helper": {
      "url": "http://localhost:3000/mcp"
    }
  }
}
```

## Local Agent Server

For using with local models (Gemma 4B, etc.):

```bash
cd agent
# Install dependencies
pip install -e .
# Or use uv
uv run python server.py
```

The agent server runs a FastAPI app with litert_lm and registers both MCP tools and agent wrapper tools.

## Tech Stack

- **Rust**: MCP server, web scraping, HTTP API
  - Tokio (async runtime)
  - Axum (HTTP framework)
  - Reqwest (HTTP client)
  - html2text (HTML → text extraction)
  - scraper (HTML parsing)

- **Python**: Agent wrapper, local LLM integration
  - FastAPI (web server)
  - litert_lm (local model inference)
  - httpx (MCP client)

## License

MIT
