#!/bin/bash
# Test script to find phone number from sekor.eu.org using llm-helper server

set -e

echo "=== Finding Phone Number from sekor.eu.org ==="
echo ""

# Step 1: Start the llm-helper server
echo "Step 1: Starting llm-helper server..."
./target/release/llm-helper --port 3000 > /tmp/server.log 2>&1 &
SERVER_PID=$!
sleep 2
echo "Server started (PID: $SERVER_PID)"
echo ""

# Step 2: Browse main page to discover all links
echo "Step 2: Browsing https://sekor.eu.org to find links..."
CONTACT_LINK=$(curl -s -X POST http://127.0.0.1:3000/browse \
  -H "Content-Type: application/json" \
  -d '{"url": "https://sekor.eu.org", "extract_text": false}' | \
  python3 -c "
import json,sys
d=json.load(sys.stdin)
for link in d.get('links', []):
    if 'contact' in link.lower():
        print(link)
        break
")

echo "Found contact page: $CONTACT_LINK"
echo ""

# Step 3: Browse contact page to extract text content
echo "Step 3: Browsing contact page to extract text..."
PHONE=$(curl -s -X POST http://127.0.0.1:3000/browse \
  -H "Content-Type: application/json" \
  -d "{\"url\": \"$CONTACT_LINK\", \"extract_text\": true}" | \
  python3 -c "
import json,sys,re
d=json.load(sys.stdin)
text = d.get('text_content', '')
# Search for phone pattern: +49 (0)159 0268 1236
phones = re.findall(r'\+\d[\d\s\-\(\)]{7,20}', text)
for p in phones:
    # Clean and check if it's a valid phone (at least 10 digits)
    digits = re.sub(r'\D', '', p)
    if len(digits) >= 10:
        print(p)
        break
")

echo ""
echo "=== RESULT ==="
echo "Phone number found: $PHONE"
echo ""

# Step 4: Cleanup
echo "Step 4: Stopping server..."
kill $SERVER_PID 2>/dev/null || true
echo "Done!"
