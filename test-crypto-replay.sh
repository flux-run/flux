#!/usr/bin/env bash
set -e

echo "Building flux logic..."
cargo build --bin flux-runtime --bin flux

echo "Killing existing instances..."
pkill -f "flux dev test-crypto" || true

echo "Starting dev environment in standalone mode..."
./target/debug/flux dev test-crypto.js --port 8999 &
DEV_PID=$!
sleep 2

echo "Making request..."
RESP_HEADERS=$(curl -s -i -X POST http://localhost:8999/test-crypto -H "Content-Type: application/json" -d '{}')
echo "$RESP_HEADERS"

EXEC_ID=$(echo "$RESP_HEADERS" | grep -i x-flux-execution-id | awk '{print $2}' | tr -d '\r')

if [ -z "$EXEC_ID" ]; then
    echo "No x-flux-execution-id found!"
    kill $DEV_PID || true
    exit 1
fi

echo "Saved execution id: $EXEC_ID"
echo "Replaying trace..."
./target/debug/flux replay $EXEC_ID --diff

kill $DEV_PID || true
echo "Done!"
