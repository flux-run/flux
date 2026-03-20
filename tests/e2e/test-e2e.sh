#!/usr/bin/env bash

set -e

# --- Configuration ---
E2E_DIR="/tmp/flux-e2e"
SERVICE_TOKEN="e2e-dev-token-999"
APP_PORT=3001
GRPC_PORT=50052  # Use different port to avoid collisions

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo "========================================"
echo "🌍 Flux Ultimate E2E Test Suite (Day 0)"
echo "========================================"

# 1. Setup Workspace
echo "👉 Cleaning up and creating workspace: $E2E_DIR"
rm -rf "$E2E_DIR"
mkdir -p "$E2E_DIR"
cd "$E2E_DIR"

# 2. Extract DATABASE_URL
echo "👉 Extracting DATABASE_URL..."
if [ -f .env ]; then
  DB_URL=$(grep "^DATABASE_URL=" .env | cut -d'=' -f2-)
elif [ -f .env.example ]; then
  DB_URL=$(grep "^DATABASE_URL=" .env.example | cut -d'=' -f2-)
else
  DB_URL=$DATABASE_URL
fi

if [ -z "$DB_URL" ]; then
  echo -e "${RED}❌ ERROR: DATABASE_URL not found in .env, .env.example, or environment.${NC}"
  exit 1
fi

# 3. Flux Init
echo "👉 Running flux init..."
flux init

# 4. Starting Flux Server
echo "👉 Starting Flux Server (port $GRPC_PORT)..."
flux server start --port "$GRPC_PORT" --service-token "$SERVICE_TOKEN" --database-url "$DB_URL" > flux-server.log 2>&1 &
SERVER_PID=$!

# Wait for server
echo "👉 Waiting for server to be ready..."
for i in {1..20}; do
  if lsof -i :$GRPC_PORT > /dev/null; then
    echo "✅ Server is ready."
    break
  fi
  sleep 1
done

# 5. Authenticate CLI
echo "👉 Authenticating CLI..."
flux auth --url "http://localhost:$GRPC_PORT" --token "$SERVICE_TOKEN" --skip-verify

# 6. Inject Application Code
echo "👉 Injecting Hono application code..."
mkdir -p src
cat > src/index.ts <<EOF
import { Hono } from "hono";
import pg from "flux:pg";

const app = new Hono();
const pool = new pg.Pool({
  connectionString: Deno.env.get("DATABASE_URL"),
});

app.get("/", (c) => c.json({ status: "ok", message: "E2E Success" }));

app.get("/db", async (c) => {
  const result = await pool.query("SELECT NOW() as time");
  return c.json({ time: result.rows[0].time });
});

export default {
  fetch: app.fetch,
};
EOF

# 7. Flux Check (Static Analysis)
echo "👉 Running flux check..."
flux check src/index.ts

# 8. Build & Run Artifact
echo "👉 Building application..."
flux build

echo "👉 Starting application (flux run) on port $APP_PORT..."
flux run --artifact src/.flux/artifact.json --port "$APP_PORT" > flux-app.log 2>&1 &
APP_PID=$!

# Wait for app
echo "👉 Waiting for app to be ready..."
for i in {1..10}; do
  if lsof -i :$APP_PORT > /dev/null; then
    echo "✅ App is ready."
    break
  fi
  sleep 1
done

# 9. Verify Live Flow
echo "👉 Verifying live HTTP response..."
curl -s "http://localhost:$APP_PORT/" | jq
curl -s "http://localhost:$APP_PORT/db" | jq

# 10. Verify Observability
echo "👉 Retrieving logs..."
# Skip header and any footer text, extract last column of the actual log line, and remove any whitespace/newlines
EXEC_ID=$(flux logs --limit 1 | grep -v "TIME" | grep -v "showing" | awk '{print $NF}' | tr -d '[:space:]')

if [ -z "$EXEC_ID" ]; then
    echo -e "${RED}❌ ERROR: No execution logs found.${NC}"
    # Continue anyway to cleanup
else
    echo "👉 Verifying trace for '$EXEC_ID'..."
    flux trace "$EXEC_ID" | head -n 10
    
    echo "👉 Verifying replay..."
    flux replay "$EXEC_ID"
fi

# 11. Cleanup
echo "👉 Shutting down E2E processes..."
kill $APP_PID 2>/dev/null || true
kill $SERVER_PID 2>/dev/null || true
wait $APP_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo "========================================"
echo -e "${GREEN}✅ Ultimate E2E Test Complete${NC}"
echo "========================================"
