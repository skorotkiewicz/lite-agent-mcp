#!/usr/bin/env python3
"""
MCP Client - Native wrapper for our llm-helper MCP server.
Usage: python3 mcp_client.py <tool_name> [arguments_json]
"""

import json
import sys
import urllib.request
import urllib.error

MCP_URL = "http://localhost:3000/mcp"


def initialize():
    """Initialize MCP session and return session ID."""
    req = urllib.request.Request(
        MCP_URL,
        data=json.dumps({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "mcp-client", "version": "1.0"}
            }
        }).encode(),
        headers={"Content-Type": "application/json"}
    )
    
    with urllib.request.urlopen(req) as resp:
        # Get session ID from headers
        session_id = resp.headers.get("mcp-session-id", "")
        result = json.loads(resp.read())
        return session_id, result


def call_tool(session_id, tool_name, arguments):
    """Call an MCP tool."""
    req = urllib.request.Request(
        MCP_URL,
        data=json.dumps({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        }).encode(),
        headers={
            "Content-Type": "application/json",
            "mcp-session-id": session_id
        }
    )
    
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 mcp_client.py <tool_name> [arguments_json]", file=sys.stderr)
        print("Tools: browse_web, search_web, extract_links", file=sys.stderr)
        sys.exit(1)
    
    tool_name = sys.argv[1]
    arguments = {}
    
    if len(sys.argv) > 2:
        arguments = json.loads(sys.argv[2])
    
    try:
        session_id, init_result = initialize()
        
        if "error" in init_result:
            print(json.dumps(init_result), file=sys.stderr)
            sys.exit(1)
        
        result = call_tool(session_id, tool_name, arguments)
        
        # Extract text content from result
        if "result" in result and "content" in result["result"]:
            for item in result["result"]["content"]:
                if item.get("type") == "text":
                    print(item.get("text", ""))
        else:
            print(json.dumps(result, indent=2))
            
    except urllib.error.URLError as e:
        print(f"MCP Server error: {e}", file=sys.stderr)
        print("Is llm-helper running on port 3000?", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
