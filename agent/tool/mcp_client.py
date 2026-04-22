"""MCP client tool for connecting to llm-helper server."""

import httpx

MCP_URL = "http://192.168.0.148:3000/mcp"
DEFAULT_TIMEOUT = 30


class MCPClient:
    """Simple MCP HTTP client with session management."""

    def __init__(self, url: str = MCP_URL):
        self.url = url
        self.session_id: str | None = None
        self._initialized = False

    def _send(self, payload: dict) -> dict:
        headers = {"Content-Type": "application/json"}
        if self.session_id:
            headers["mcp-session-id"] = self.session_id

        with httpx.Client(timeout=DEFAULT_TIMEOUT) as client:
            response = client.post(self.url, json=payload, headers=headers)
            response.raise_for_status()
            # Capture session ID from response if present
            new_session = response.headers.get("mcp-session-id")
            if new_session:
                self.session_id = new_session
            return response.json()

    def initialize(self) -> None:
        """Initialize MCP session."""
        if self._initialized:
            return
        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "clientInfo": {"name": "litert-agent", "version": "1.0"},
                "capabilities": {"tools": {}},
            },
        }
        resp = self._send(payload)
        if "error" in resp:
            raise RuntimeError(f"MCP init failed: {resp['error']}")
        self._initialized = True

    def call_tool(self, name: str, arguments: dict) -> str:
        """Call an MCP tool."""
        self.initialize()
        payload = {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }
        resp = self._send(payload)
        if "error" in resp:
            return f"MCP error: {resp['error']}"
        result = resp.get("result", {})
        content = result.get("content", [])
        for item in content:
            if item.get("type") == "text":
                return item.get("text", "")
        return str(result)


# Global client instance
_mcp = MCPClient()


def mcp_browse_web(url: str, extract_text: bool = True) -> str:
    """STEP 1 and STEP 5: Browse a web page.

    Use this FIRST when asked to find information on a website.
    After getting the result, check if the answer is there.
    If NOT found, you MUST call mcp_extract_links next (STEP 3).
    If promising links were found, you MUST call mcp_browse_web again on those links (STEP 5).
    NEVER stop after just one call to this function.

    Args:
        url: The URL to fetch
        extract_text: Whether to extract readable text (default True)

    Returns:
        Page content with title, text, links count, images count
    """
    return _mcp.call_tool("browse_web", {"url": url, "extract_text": extract_text})


def mcp_extract_links(url: str) -> str:
    """STEP 3: Extract all links from a web page.

    Use this ONLY when mcp_browse_web did NOT find the answer.
    Look for promising links like /contact, /about, /info in the result.
    After this, you MUST call mcp_browse_web on at least one promising link.
    This is STEP 3 of the mandatory 5-step research protocol.

    Args:
        url: The URL to extract links from

    Returns:
        List of all links found on the page
    """
    return _mcp.call_tool("extract_links", {"url": url})


def mcp_search_web(query: str, limit: int = 5) -> str:
    """Search the web using DuckDuckGo.

    Use this for general web searches, not for exploring a specific website.
    For website exploration, use mcp_browse_web + mcp_extract_links instead.

    Args:
        query: Search query string
        limit: Maximum number of results (default 5)

    Returns:
        Search results as formatted text
    """
    return _mcp.call_tool("search_web", {"query": query, "limit": limit})
