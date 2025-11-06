#!/bin/bash
# Load testing script for Octopus API Gateway
# Requires: wrk or hey or ab (Apache Bench)

set -e

GATEWAY_URL="${GATEWAY_URL:-http://localhost:8080}"
DURATION="${DURATION:-30s}"
CONNECTIONS="${CONNECTIONS:-100}"
THREADS="${THREADS:-4}"

echo "🚀 Octopus API Gateway Load Test"
echo "=================================="
echo "Gateway URL: $GATEWAY_URL"
echo "Duration: $DURATION"
echo "Connections: $CONNECTIONS"
echo "Threads: $THREADS"
echo ""

# Check if wrk is available
if command -v wrk &> /dev/null; then
    echo "Using wrk for load testing..."
    echo ""
    
    echo "📊 Test 1: Simple GET requests"
    wrk -t"$THREADS" -c"$CONNECTIONS" -d"$DURATION" "$GATEWAY_URL/health"
    
    echo ""
    echo "📊 Test 2: GET with headers"
    wrk -t"$THREADS" -c"$CONNECTIONS" -d"$DURATION" \
        -H "Accept-Encoding: gzip" \
        -H "User-Agent: LoadTest/1.0" \
        "$GATEWAY_URL/health"
    
    echo ""
    echo "📊 Test 3: Authenticated requests (JWT)"
    # Generate a test token
    TOKEN="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ0ZXN0LXVzZXIiLCJleHAiOjk5OTk5OTk5OTl9.fake_signature"
    wrk -t"$THREADS" -c"$CONNECTIONS" -d"$DURATION" \
        -H "Authorization: Bearer $TOKEN" \
        "$GATEWAY_URL/api/test"
    
elif command -v hey &> /dev/null; then
    echo "Using hey for load testing..."
    echo ""
    
    echo "📊 Test 1: Simple GET requests"
    hey -z "$DURATION" -c "$CONNECTIONS" "$GATEWAY_URL/health"
    
    echo ""
    echo "📊 Test 2: GET with compression"
    hey -z "$DURATION" -c "$CONNECTIONS" \
        -H "Accept-Encoding: gzip" \
        "$GATEWAY_URL/health"
    
elif command -v ab &> /dev/null; then
    echo "Using Apache Bench for load testing..."
    echo ""
    
    REQUESTS=10000
    echo "📊 Test 1: Simple GET requests ($REQUESTS total)"
    ab -n "$REQUESTS" -c "$CONNECTIONS" "$GATEWAY_URL/health"
    
    echo ""
    echo "📊 Test 2: GET with headers"
    ab -n "$REQUESTS" -c "$CONNECTIONS" \
        -H "Accept-Encoding: gzip" \
        "$GATEWAY_URL/health"
    
else
    echo "❌ No load testing tool found!"
    echo "Please install one of: wrk, hey, or ab (Apache Bench)"
    echo ""
    echo "Install wrk: brew install wrk (macOS) or build from source"
    echo "Install hey: go install github.com/rakyll/hey@latest"
    echo "Install ab: Usually comes with Apache (httpd-tools package)"
    exit 1
fi

echo ""
echo "✅ Load testing complete!"
echo ""
echo "📈 To interpret results:"
echo "  - Requests/sec: Higher is better (target: 10,000+)"
echo "  - Latency p99: Lower is better (target: <10ms)"
echo "  - Errors: Should be 0%"
echo "  - Transfer/sec: Shows bandwidth usage"

