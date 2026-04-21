# LLM Helper Server

A Rust-based server that helps LLMs (like Claude, GPT-4, etc.) with difficult tasks like browsing webpages and searching the web. Implements the Model Context Protocol (MCP) for seamless integration with compatible LLM clients.

## Features

- **Web Browsing**: Fetch and extract content from any webpage
- **Web Search**: Search the web using DuckDuckGo
- **Link Extraction**: Extract all links from a webpage
- **MCP Protocol Support**: Compatible with MCP-enabled clients (e.g., Claude Desktop)
- **HTTP API**: REST API for direct integration

## Building

```bash
cargo build --release
```

## Running

### HTTP Server Mode

```bash
./target/release/llm-helper --port 3000
```

The server will be available at `http://127.0.0.1:3000`

### MCP Stdio Mode (for Claude Desktop)

```bash
./target/release/llm-helper --mcp-stdio
```

## HTTP API Endpoints

- `GET /health` - Health check
- `POST /browse` - Browse a webpage
  - Body: `{"url": "https://example.com", "extract_text": true}`
- `POST /search` - Search the web
  - Body: `{"query": "rust programming", "limit": 5}`
- `GET /mcp/tools` - List available MCP tools
- `POST /mcp/invoke` - Invoke an MCP tool

## MCP Tools

The server provides the following MCP tools:

1. **browse_web** - Fetch and extract content from a webpage
   - Parameters: `url` (string), `extract_text` (boolean, optional)

2. **search_web** - Search the web for information
   - Parameters: `query` (string), `limit` (integer, optional)

3. **extract_links** - Extract all links from a webpage
   - Parameters: `url` (string)

4. **smart_search** - Intelligently search a website for specific information
   - Parameters: `url` (string), `query` (string), `max_depth` (integer, optional)
   - Recursively follows relevant pages to find the query
   - **Tip**: Use focused keywords (e.g., "contact phone") rather than full sentences for best results

## Claude Desktop Configuration

Add to your Claude Desktop configuration file (`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS or equivalent):

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

## Example Usage

### Browse a webpage

```bash
curl -X POST http://127.0.0.1:3000/browse \
  -H "Content-Type: application/json" \
  -d '{"url": "https://www.rust-lang.org"}'
```

### Search the web

```bash
curl -X POST http://127.0.0.1:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": "rust programming", "limit": 3}'
```

### Smart search a website

```bash
curl -X POST http://127.0.0.1:3000/mcp/invoke \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
      "name": "smart_search",
      "arguments": {
        "url": "https://example.com",
        "query": "contact phone",
        "max_depth": 3
      }
    }
  }'
```

## Architecture

The server is built with:
- **Tokio**: Async runtime
- **Axum**: HTTP server framework
- **Reqwest**: HTTP client for web requests
- **Scraper**: HTML parsing
- **Serde**: JSON serialization
- **Tracing**: Logging

## License

MIT
