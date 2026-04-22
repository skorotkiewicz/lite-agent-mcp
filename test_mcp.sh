#!/bin/bash
timeout 10 ./target/release/llm-helper --mcp-stdio <<< $'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"smart_search","arguments":{"url":"https://sekor.eu.org","query":"simple backup","max_depth":10}}}' 2>&1 | less #grep "Sending:" | tail -1 | sed 's/.*Sending: //' | jq .

S=$(curl -s -X POST http://localhost:3000/mcp -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0.0"}}}' -D - | grep -i mcp-session-id | awk '{print $2}' | tr -d '\r')

curl -s -X POST http://localhost:3000/mcp -H "Content-Type: application/json" -H "mcp-session-id: $S" -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"smart_search","arguments":{"url":"https://sekor.eu.org","query":"phone number","max_depth":10}}}' | jq .
