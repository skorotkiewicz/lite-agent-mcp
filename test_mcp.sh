#!/bin/bash
set -e

# Native MCP test using mcp_client.py
# No curl needed - uses Python urllib directly

PORT=3000
SERVER_PID=""

cleanup() {
    if [ -n "$SERVER_PID" ]; then
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Kill any existing server
lsof -ti :$PORT | xargs kill -9 2>/dev/null || true
sleep 1

# Start server
./target/release/llm-helper &
SERVER_PID=$!
sleep 2

MCP="python3 /home/mod/Dev/React/firefox-ai/llm-helper/mcp_client.py"

echo "=== Test 1: Browse homepage ==="
$MCP browse_web '{"url":"https://sekor.eu.org"}' | head -15

echo ""
echo "=== Test 2: Extract links ==="
$MCP extract_links '{"url":"https://sekor.eu.org"}' | head -10

echo ""
echo "=== Test 3: Find phone number ==="
$MCP browse_web '{"url":"https://sekor.eu.org/contact"}' | grep -A1 "Phone"

echo ""
echo "=== Test 4: Find simple backup ==="
$MCP browse_web '{"url":"https://sekor.eu.org/archive/2016/01/"}' | grep -i backup

echo ""
echo "=== Test 5: Get backup commands ==="
$MCP browse_web '{"url":"https://sekor.eu.org/techlog/simple-server-backup"}' | grep -E "^tar|^mysqldump|^rsync|^split" | head -5

echo ""
echo "=== All tests passed! ==="
