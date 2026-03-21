#!/usr/bin/env bash

set -e

# Use local build binaries
export PATH="$(pwd)/target/debug:$PATH"

# Allow local connections for demo
export FLUXBASE_ALLOW_LOOPBACK_POSTGRES=1
export FLUXBASE_ALLOW_LOOPBACK_FETCH=1

# Export env vars for server and CLI
export FLUX_SERVICE_TOKEN="dev-token-123"

# Kill anything on port 50051 and 3000 to ensure a clean start
echo "👉 Cleaning up existing processes..."
lsof -ti :50051 | xargs kill -9 2>/dev/null || true
lsof -ti :3000 | xargs kill -9 2>/dev/null || true

# Extract DB URL
echo "👉 Extracting DATABASE_URL..."
if [ -f .env ]; then
  DB_URL=$(grep "^DATABASE_URL=" .env | cut -d'=' -f2-)
elif [ -f /Users/shashisharma/code/my-app/.env ]; then
  DB_URL=$(grep "^DATABASE_URL=" /Users/shashisharma/code/my-app/.env | cut -d'=' -f2-)
elif [ -f .env.example ]; then
  DB_URL=$(grep "^DATABASE_URL=" .env.example | cut -d'=' -f2-)
else
  DB_URL=$DATABASE_URL
fi

if [ -z "$DB_URL" ]; then
  echo -e "${RED}❌ ERROR: DATABASE_URL not found in .env, .env.example, or environment.${NC}"
  exit 1
fi

export DATABASE_URL="$DB_URL"

echo "👉 Starting Flux Server in background..."
flux server start --service-token "$FLUX_SERVICE_TOKEN" --database-url "$DB_URL" > /tmp/flux-server.log 2>&1 &
SERVER_PID=$!

# Wait for server to be ready (port 50051)
echo "👉 Waiting for server (50051) to be ready..."
for i in {1..10}; do
  if lsof -i :50051 > /dev/null; then
    echo "✅ Server is ready."
    break
  fi
  sleep 1
done

# Authenticate CLI to local server
echo "👉 Authenticating CLI to local server..."
flux auth --url http://localhost:50051 --token "$FLUX_SERVICE_TOKEN" --skip-verify

echo "👉 Building Application..."
flux build > /dev/null

echo "👉 Starting Application (flux run) in background..."
flux run --artifact src/.flux/artifact.json --port 3000 > /tmp/flux-app.log 2>&1 &
APP_PID=$!

# Wait for app to be ready (port 3000)
echo "👉 Waiting for app (3000) to be ready..."
for i in {1..10}; do
  if lsof -i :3000 > /dev/null; then
    echo "✅ App is ready."
    break
  fi
  sleep 1
done

BASE_URL="http://localhost:3000"

echo "========================================"
echo "🚀 Flux Demo Test Flow"
echo "========================================"

echo ""
echo "1️⃣ Health Check"
curl -s "$BASE_URL/" | jq
echo ""

echo "----------------------------------------"
echo "2️⃣ Create Order (SUCCESS FLOW)"
echo "----------------------------------------"

RESPONSE=$(curl -i -s -X POST "$BASE_URL/orders" \
  -H "content-type: application/json" \
  -d '{"email":"user@example.com","amount":100}')

echo "$RESPONSE"

# Extract execution ID
EXEC_ID=$(echo "$RESPONSE" | grep -i "x-flux-execution-id" | awk '{print $2}' | tr -d '\r' | tr -d '[:space:]')

echo ""
echo "🧠 Execution ID: $EXEC_ID"

echo ""
echo "----------------------------------------"
echo "3️⃣ Fetch Orders"
echo "----------------------------------------"

curl -s "$BASE_URL/orders" | jq
echo ""

echo ""
echo "----------------------------------------"
echo "4️⃣ Replay (SAFE — no side effects)"
echo "----------------------------------------"

echo "👉 Running: flux replay $EXEC_ID"
flux replay "$EXEC_ID"

echo ""
echo "----------------------------------------"
echo "5️⃣ Replay Again (should be identical)"
echo "----------------------------------------"

flux replay "$EXEC_ID"

# --- Step 6: Verify DB Idempotency (Automated) ---
echo ""
echo "----------------------------------------"
echo "6️⃣ Verify DB Idempotency (Automated)"
echo "----------------------------------------"
echo "👉 Capturing COUNT BEFORE..."
COUNT_BEFORE=$(flux exec src/count-orders.ts | grep "COUNT:" | awk '{print $NF}')
echo "   Count: $COUNT_BEFORE"

echo "👉 Replaying $EXEC_ID..."
flux replay "$EXEC_ID" > /dev/null

echo "👉 Capturing COUNT AFTER..."
COUNT_AFTER=$(flux exec src/count-orders.ts | grep "COUNT:" | awk '{print $NF}')
echo "   Count: $COUNT_AFTER"

if [ "$COUNT_BEFORE" -eq "$COUNT_AFTER" ]; then
  echo -e "${GREEN}✅ SUCCESS: Replay was side-effect free (idempotent).${NC}"
else
  echo -e "${RED}❌ FAILURE: DB count increased! Replay is NOT idempotent.${NC}"
  exit 1
fi

echo ""
echo "----------------------------------------"
echo "7️⃣ Failure Flow"
echo "----------------------------------------"

FAIL_RESPONSE=$(curl -i -s "$BASE_URL/fail" || true)

echo "$FAIL_RESPONSE"

FAIL_EXEC_ID=$(echo "$FAIL_RESPONSE" | grep -i "x-flux-execution-id" | awk '{print $2}' | tr -d '\r')

echo ""
echo "💥 Failure Execution ID: $FAIL_EXEC_ID"

echo ""
echo "----------------------------------------"
echo "8️⃣ Why did it fail?"
echo "----------------------------------------"

flux why "$FAIL_EXEC_ID"

echo ""
echo "----------------------------------------"
echo "9️⃣ Replay Failed Execution"
echo "----------------------------------------"

flux replay "$FAIL_EXEC_ID" || true

echo ""
echo "----------------------------------------"
echo "🔟 Resume Failed Execution"
echo "----------------------------------------"

echo "👉 Running: flux resume $FAIL_EXEC_ID"
flux resume "$FAIL_EXEC_ID" || true

echo ""
echo "----------------------------------------"
echo "1️⃣1️⃣ Trace Verbose (Check Decoding)"
echo "----------------------------------------"

echo "👉 Running: flux trace $EXEC_ID --verbose"
flux trace "$EXEC_ID" --verbose | grep -A 5 "response" | head -n 20

echo ""
echo "----------------------------------------"
echo "1️⃣2️⃣ Logs Verification"
echo "----------------------------------------"
echo "👉 Running: flux logs $EXEC_ID"
flux logs "$EXEC_ID" | head -n 10

echo ""
echo "----------------------------------------"
echo "1️⃣3️⃣ Tail Verification (Live Stream)"
echo "----------------------------------------"
# Test tail by running in background, hitting an endpoint, and checking output
TEMP_TAIL_OUT=$(mktemp)
flux tail > "$TEMP_TAIL_OUT" 2>&1 &
TAIL_PID=$!

echo "👉 Hit endpoint while tailing..."
sleep 2
curl -s http://localhost:3000/ > /dev/null
sleep 2

kill $TAIL_PID 2>/dev/null || true
echo "👉 Tail Summary (last 5 lines):"
tail -n 5 "$TEMP_TAIL_OUT"
rm "$TEMP_TAIL_OUT"

echo ""
echo "----------------------------------------"
echo "1️⃣4️⃣ Static Analysis (flux check)"
echo "----------------------------------------"
flux check src/index.ts

echo ""
echo "----------------------------------------"
echo "1️⃣5️⃣ Exec One-off Script"
echo "----------------------------------------"

echo "👉 Running: flux exec src/check-db.ts"
flux exec src/check-db.ts

echo "----------------------------------------"
echo "1️⃣6️⃣ Divergence Test (Server-side Replay)"
echo "----------------------------------------"
echo "👉 Modifying code to add 'v': 'v2' to response..."
sed -i '' 's/status: "completed"/status: "completed", v: "v2"/g' src/index.ts
flux build > /dev/null

echo "👉 Replaying $EXEC_ID using SERVER-SIDE replay (uses recorded code)..."
echo "👉 Note: This will NOT show the 'v2' changes because it uses the historical artifact."
flux replay "$EXEC_ID"

echo "----------------------------------------"
echo "1️⃣7️⃣ Local Replay (New Feature!)"
echo "----------------------------------------"
echo "👉 Running LOCAL code against $EXEC_ID history..."
echo "👉 Note: This WILL show the 'v2' changes because it uses your local built artifact!"
flux run --replay "$EXEC_ID" --artifact src/.flux/artifact.json

echo "👉 Reverting code change..."
sed -i '' 's/, v: "v2"//g' src/index.ts
flux build > /dev/null

echo "👉 Shutting down Application (PID $APP_PID)..."
kill $APP_PID 2>/dev/null || true
wait $APP_PID 2>/dev/null || true

echo "👉 Shutting down Flux Server (PID $SERVER_PID)..."
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo ""
echo "========================================"
echo "✅ Test Flow Complete"
echo "========================================"

echo ""
echo "👉 Useful commands:"
echo "flux trace $EXEC_ID --verbose"
echo "flux replay $EXEC_ID --commit"
echo "flux resume $FAIL_EXEC_ID"
echo "flux why $FAIL_EXEC_ID"
echo "flux exec src/check-db.ts"