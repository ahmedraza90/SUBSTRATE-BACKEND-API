#!/bin/bash

# Test script for the Chain A Backend API

BASE_URL="http://127.0.0.1:3001"

echo "🚀 Testing Chain A Backend API..."
echo

# Test 1: Health check
echo "1. Testing health endpoint..."
curl -s -X GET "${BASE_URL}/health" | jq . || echo "Health check failed or jq not available"
echo -e "\n"

# Test 2: Get storage
echo "2. Testing get-storage endpoint..."
curl -s -X GET "${BASE_URL}/get-storage" | jq . || echo "Get storage failed or jq not available"
echo -e "\n"

# Test 3: Get latest events
echo "3. Testing latest-events endpoint..."
curl -s -X GET "${BASE_URL}/latest-events" | jq . || echo "Latest events failed or jq not available"
echo -e "\n"

# Test 4: Do something transaction
echo "4. Testing do-something endpoint with value 42..."
curl -s -X POST "${BASE_URL}/do-something" \
  -H "Content-Type: application/json" \
  -d '{"value": 42, "signer": "//Alice"}' | jq . || echo "Do something failed or jq not available"
echo -e "\n"

# Test 5: Do something transaction with different value
echo "5. Testing do-something endpoint with value 100..."
curl -s -X POST "${BASE_URL}/do-something" \
  -H "Content-Type: application/json" \
  -d '{"value": 100}' | jq . || echo "Do something failed or jq not available"
echo -e "\n"

echo "✅ All tests completed!"
