# LLM Helper Server

A minimal MCP (Model Context Protocol) server that helps LLMs browse webpages and search the web. Implements the MCP protocol for seamless integration with compatible LLM clients.

## Features

- **MCP Protocol**: Full MCP support via HTTP POST
- **Web Browsing**: Fetch and extract content from any webpage
- **Web Search**: Search the web using DuckDuckGo
- **Smart Search**: Intelligently search websites for specific information

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
      "name": "smart_search",
      "arguments": {
        "url": "https://example.com",
        "query": "contact phone",
        "max_depth": 10
      }
    }
  }'
```

## MCP Tools

1. **browse_web** - Fetch and extract content from a webpage
   - Parameters: `url` (string), `extract_text` (boolean, optional)

2. **search_web** - Search the web for information
   - Parameters: `query` (string), `limit` (integer, optional)

3. **extract_links** - Extract all links from a webpage
   - Parameters: `url` (string)

4. **smart_search** - Intelligently search a website for specific information
   - Parameters: `url` (string), `query` (string), `max_depth` (integer, optional)
   - Recursively follows relevant pages to find the query

## MCP Client Configuration

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

## Architecture

- **Tokio**: Async runtime
- **Axum**: HTTP server framework
- **Reqwest**: HTTP client for web requests
- **MCP**: Model Context Protocol implementation

## License

MIT
