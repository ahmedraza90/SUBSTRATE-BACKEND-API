#!/bin/bash

# Simple API test for Chain A Backend

echo "=== Chain A Backend API Test ==="
echo

echo "1. Health Check:"
curl -s -X GET "http://127.0.0.1:3001/health"
echo
echo

echo "2. Get Storage Value:"
curl -s -X GET "http://127.0.0.1:3001/get-storage"
echo
echo

echo "3. Get Latest Events:"
curl -s -X GET "http://127.0.0.1:3001/latest-events"
echo
echo

echo "4. Test Transaction (may fail due to nonce issues):"
curl -s -X POST "http://127.0.0.1:3001/do-something" \
  -H "Content-Type: application/json" \
  -d '{"value": 123}'
echo
echo

echo "=== Test Complete ==="
