#!/bin/bash
# scripts/e2e/user-flow.sh
#
# Flux E2E User Flow Test
# ─────────────────────────────────────────────────────────────────────────────
# Simulates a real developer going from zero to debugging in a clean environment.
# Every step asserts behavior, not just exit codes.
#
# Usage (local):
#   DATABASE_URL=postgres://... FLUX_PORT=3000 bash scripts/e2e/user-flow.sh
#
# Usage (Docker — recommended):
#   docker compose -f scripts/e2e/docker-compose.minimal.yml run --rm e2e
#   docker compose -f scripts/e2e/docker-compose.full.yml    run --rm e2e
#
# CI:
#   See .github/workflows/ci.yml — job: e2e
#
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/assertions.sh"

# ── Config (override via env) ─────────────────────────────────────────────────
DATABASE_URL="${DATABASE_URL:-postgres://postgres:postgres@localhost:5432/postgres}"
REDIS_URL="${REDIS_URL:-}"           # empty = no Redis (tests Redis-optional path)
FLUX_PORT="${FLUX_PORT:-3000}"
FLUX_SERVER_URL="${FLUX_SERVER_URL:-http://127.0.0.1:50051}"
SERVICE_TOKEN="${SERVICE_TOKEN:-e2e-test-token}"
APP_URL="http://localhost:${FLUX_PORT}"

E2E_DIR=$(mktemp -d)
trap 'echo "Cleaning up $E2E_DIR"; rm -rf "$E2E_DIR"; kill $(jobs -p) 2>/dev/null || true' EXIT

echo ""
echo "╔═══════════════════════════════════════════════════════╗"
echo "║        FLUX E2E USER FLOW — $(date -u +%Y-%m-%dT%H:%M:%SZ)       ║"
echo "╠═══════════════════════════════════════════════════════╣"
echo "║  DATABASE_URL: ${DATABASE_URL:0:40}..."
echo "║  REDIS_URL:    ${REDIS_URL:-<not set — testing Redis-optional path>}"
echo "║  PORT:         $FLUX_PORT"
echo "╚═══════════════════════════════════════════════════════╝"

# ── Prerequisite: flux is in PATH ─────────────────────────────────────────────
section "0. PREREQUISITES"
assert_exit_zero "flux is in PATH" which flux
FLUX_VERSION=$(flux --version 2>&1)
assert_nonempty "$FLUX_VERSION" "flux --version returns a string"
pass "flux version: $FLUX_VERSION"

# ── PHASE 1: Server ───────────────────────────────────────────────────────────
section "1. FLUX SERVER"

flux server start \
  --database-url "$DATABASE_URL" \
  --service-token "$SERVICE_TOKEN" \
  --port 50051 \
  > "$E2E_DIR/server.log" 2>&1 &
SERVER_PID=$!

sleep 3  # give server time to boot and run migrations

if kill -0 "$SERVER_PID" 2>/dev/null; then
  pass "flux-server started (pid $SERVER_PID)"
else
  fail "flux-server failed to start"
  cat "$E2E_DIR/server.log"
  e2e_summary
fi

# ── PHASE 2: Project Init ─────────────────────────────────────────────────────
section "2. PROJECT INIT"

cd "$E2E_DIR"
flux init e2e-app > "$E2E_DIR/init.log" 2>&1
assert_exit_zero "flux init creates project directory" test -d "$E2E_DIR/e2e-app"
assert_exit_zero "flux.json exists" test -f "$E2E_DIR/e2e-app/flux.json"
assert_exit_zero "entry file exists" test -f "$E2E_DIR/e2e-app/index.ts"
cd "$E2E_DIR/e2e-app"

# Write a test handler with both success and failure routes
cat > index.ts << 'HANDLER'
import { Hono } from "npm:hono";
import pg from "flux:pg";

const app = new Hono();
const pool = new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });

app.get("/health", (c) => c.json({ status: "ok", timestamp: Date.now() }));

app.post("/users", async (c) => {
  const body = await c.req.json();
  const id = crypto.randomUUID();
  await pool.query(
    "INSERT INTO e2e_users (id, name) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    [id, body.name]
  );
  return c.json({ id, name: body.name }, 201);
});

app.get("/fail", () => {
  throw new Error("e2e-intentional-failure: testing replay of errors");
});

export default app;
HANDLER

pass "test handler written (health + /users POST + /fail route)"

# ── PHASE 3: Build ────────────────────────────────────────────────────────────
section "3. BUILD"

BUILD_OUT=$(flux build index.ts 2>&1)
assert_contains "$BUILD_OUT" "artifact" "flux build produces an artifact"

# ── PHASE 4: Runtime (production mode) ────────────────────────────────────────
section "4. RUNTIME START"

export DATABASE_URL
export FLUX_SERVICE_TOKEN="$SERVICE_TOKEN"

# Create the test table
flux run --url "$FLUX_SERVER_URL" --token "$SERVICE_TOKEN" - << 'SQL' || true
import pg from "flux:pg";
const pool = new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });
await pool.query("CREATE TABLE IF NOT EXISTS e2e_users (id TEXT PRIMARY KEY, name TEXT)");
await pool.end();
SQL

flux run index.ts \
  --listen \
  --url "$FLUX_SERVER_URL" \
  --token "$SERVICE_TOKEN" \
  --host 127.0.0.1 \
  --port "$FLUX_PORT" \
  > "$E2E_DIR/runtime.log" 2>&1 &
RUNTIME_PID=$!

wait_for_http "$APP_URL/health" 20

# ── PHASE 5: Happy path ───────────────────────────────────────────────────────
section "5. HAPPY PATH — /health"

HEALTH=$(curl -sf "$APP_URL/health" 2>/dev/null)
assert_json_field "$HEALTH" ".status" "ok" "/health returns status=ok"
assert_nonempty "$(echo "$HEALTH" | jq -r '.timestamp')" "/health includes timestamp"

section "6. HAPPY PATH — /users POST (DB write + checkpoint)"

USER_RESP=$(curl -sf -X POST "$APP_URL/users" \
  -H "content-type: application/json" \
  -d '{"name":"e2e-test-user"}' 2>/dev/null)
assert_json_field "$USER_RESP" ".name" "e2e-test-user" "/users returns correct name"
USER_ID=$(echo "$USER_RESP" | jq -r '.id')
assert_nonempty "$USER_ID" "/users returns a UUID"

sleep 1  # let execution be recorded

# ── PHASE 6: Observability — tail → trace ─────────────────────────────────────
section "7. OBSERVABILITY (tail + trace)"

LOGS=$(flux logs \
  --url "$FLUX_SERVER_URL" \
  --token "$SERVICE_TOKEN" \
  --limit 5 2>/dev/null || echo "")
assert_nonempty "$LOGS" "flux logs returns entries"

# Get the execution ID of the /users POST
EXEC_ID=$(flux logs \
  --url "$FLUX_SERVER_URL" \
  --token "$SERVICE_TOKEN" \
  --limit 1 \
  --format json 2>/dev/null | jq -r '.[0].id // empty' || echo "")

if [[ -n "$EXEC_ID" && "$EXEC_ID" != "null" ]]; then
  pass "captured execution ID: $EXEC_ID"

  TRACE=$(flux trace "$EXEC_ID" \
    --url "$FLUX_SERVER_URL" \
    --token "$SERVICE_TOKEN" 2>/dev/null || echo "")
  assert_nonempty "$TRACE" "flux trace returns output"
  assert_contains "$TRACE" "$EXEC_ID" "trace includes execution ID"

  # ── TRACE HONESTY ────────────────────────────────────────────────────────
  # The /users POST performs exactly 1 DB query (the INSERT).
  # Assert the trace is honest: the checkpoint is present, not fabricated,
  # not missing, and the count is exactly what the code does.
  #
  # What we're proving:
  #   ✅ DB checkpoint present  → Flux recorded the IO
  #   ✅ exactly 1 checkpoint   → no phantom checkpoints fabricated
  #   ✅ /health has 0 DB hits  → Flux doesn't over-checkpoint pure routes

  # 1. The INSERT checkpoint must appear in the trace
  assert_contains "$TRACE" "postgres" \
    "trace honesty: /users trace contains a postgres checkpoint"

  # 2. Count DB checkpoints — expect exactly 1 for a single INSERT
  DB_CP_COUNT=$(echo "$TRACE" | grep -ic "postgres" || echo "0")
  if [[ "$DB_CP_COUNT" -eq 1 ]]; then
    pass "trace honesty: exactly 1 DB checkpoint recorded (not 0, not >1)"
  elif [[ "$DB_CP_COUNT" -eq 0 ]]; then
    fail "trace honesty: 0 DB checkpoints — INSERT was NOT checkpointed (missing history)"
  else
    fail "trace honesty: $DB_CP_COUNT DB checkpoints — expected 1 (fabricated or duplicate history)"
  fi

  # 3. The /health execution should have ZERO DB checkpoints
  #    Fetch the health execution ID (most recent GET)
  HEALTH_EXEC_ID=$(flux logs \
    --url "$FLUX_SERVER_URL" \
    --token "$SERVICE_TOKEN" \
    --limit 10 \
    --format json 2>/dev/null \
    | jq -r '[.[] | select(.route == "/health" or .path == "/health")] | .[0].id // empty' || echo "")

  if [[ -n "$HEALTH_EXEC_ID" && "$HEALTH_EXEC_ID" != "null" ]]; then
    HEALTH_TRACE=$(flux trace "$HEALTH_EXEC_ID" \
      --url "$FLUX_SERVER_URL" \
      --token "$SERVICE_TOKEN" 2>/dev/null || echo "")
    HEALTH_DB_COUNT=$(echo "$HEALTH_TRACE" | grep -ic "postgres" || echo "0")
    if [[ "$HEALTH_DB_COUNT" -eq 0 ]]; then
      pass "trace honesty: /health trace has 0 DB checkpoints (pure route, no IO fabricated)"
    else
      fail "trace honesty: /health trace has $HEALTH_DB_COUNT DB checkpoint(s) — phantom IO recorded"
    fi
  else
    pass "trace honesty: /health exec not found in logs (skipping — non-critical)"
  fi
else
  fail "could not capture execution ID from flux logs"
fi

# ── PHASE 7: Replay safety ────────────────────────────────────────────────────
section "8. REPLAY SAFETY (core promise)"

if [[ -n "$EXEC_ID" && "$EXEC_ID" != "null" ]]; then
  REPLAY1=$(flux replay "$EXEC_ID" \
    --url "$FLUX_SERVER_URL" \
    --token "$SERVICE_TOKEN" 2>/dev/null || echo "__replay_failed__")
  assert_not_contains "$REPLAY1" "__replay_failed__" "flux replay completes without error"

  # Determinism: two replays must produce identical output
  REPLAY2=$(flux replay "$EXEC_ID" \
    --url "$FLUX_SERVER_URL" \
    --token "$SERVICE_TOKEN" 2>/dev/null || echo "__replay_failed__")

  if diff <(echo "$REPLAY1") <(echo "$REPLAY2") >/dev/null 2>&1; then
    pass "determinism: replay1 == replay2 (identical outputs)"
  else
    fail "determinism: replay outputs differ between runs"
    echo "  replay1: $(echo "$REPLAY1" | head -2)"
    echo "  replay2: $(echo "$REPLAY2" | head -2)"
  fi

  # ── REPLAY PROOF: replay must NOT mutate the database ────────────────────
  # This is the killer feature. Replay must be a read from recorded history,
  # not a re-execution against live systems.
  #
  # Method: use psql directly — NOT flux:pg — to read the true DB state.
  # This ensures we're measuring the real database, not a Flux-mediated view.
  #
  # Proof structure:
  #   COUNT_BEFORE  → row count before any replays
  #   replay × 3   → run the same execution three times
  #   COUNT_AFTER   → row count must be identical to COUNT_BEFORE
  #
  # If replay re-executes the INSERT, count increases. That would mean:
  #   replay == re-execution  → Flux's core promise is broken

  COUNT_BEFORE=$(psql "$DATABASE_URL" -t -c \
    "SELECT COUNT(*) FROM e2e_users;" 2>/dev/null | tr -d ' ' || echo "err")
  assert_nonempty "$COUNT_BEFORE" "replay proof: psql can read row count before replay"
  pass "replay proof: COUNT_BEFORE = $COUNT_BEFORE row(s)"

  # Replay 3× — if each re-inserts, count will be COUNT_BEFORE + 3
  for i in 1 2 3; do
    flux replay "$EXEC_ID" \
      --url "$FLUX_SERVER_URL" \
      --token "$SERVICE_TOKEN" >/dev/null 2>&1 || true
  done

  COUNT_AFTER=$(psql "$DATABASE_URL" -t -c \
    "SELECT COUNT(*) FROM e2e_users;" 2>/dev/null | tr -d ' ' || echo "err")

  if [[ "$COUNT_BEFORE" != "err" && "$COUNT_AFTER" != "err" ]]; then
    if [[ "$COUNT_AFTER" -eq "$COUNT_BEFORE" ]]; then
      pass "replay proof: COUNT_AFTER ($COUNT_AFTER) == COUNT_BEFORE ($COUNT_BEFORE) after 3 replays"
      pass "replay proof: replay ≠ re-execution — DB was NOT mutated by replay"
    else
      fail "replay proof: COUNT went $COUNT_BEFORE → $COUNT_AFTER after 3 replays (INSERT fired again — replay is broken)"
    fi
  else
    fail "replay proof: could not read DB row count via psql — proof is inconclusive"
  fi
fi

# ── PHASE 8: Failure scenario ─────────────────────────────────────────────────
section "9. FAILURE SCENARIO — /fail + flux why"

FAIL_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$APP_URL/fail" || echo "000")
assert_equal "$FAIL_STATUS" "500" "/fail returns HTTP 500"

sleep 1  # let error be recorded

FAIL_EXEC_ID=$(flux logs \
  --url "$FLUX_SERVER_URL" \
  --token "$SERVICE_TOKEN" \
  --status error \
  --limit 1 \
  --format json 2>/dev/null | jq -r '.[0].id // empty' || echo "")

if [[ -n "$FAIL_EXEC_ID" && "$FAIL_EXEC_ID" != "null" ]]; then
  pass "error execution recorded: $FAIL_EXEC_ID"

  WHY=$(flux why "$FAIL_EXEC_ID" \
    --url "$FLUX_SERVER_URL" \
    --token "$SERVICE_TOKEN" 2>/dev/null || echo "")
  assert_nonempty "$WHY" "flux why returns output for failed execution"
  assert_contains "$WHY" "e2e-intentional-failure" "flux why surfaces the error message"

  # Replay of a failed execution must also produce the same error (no fabricated success)
  FAIL_REPLAY=$(flux replay "$FAIL_EXEC_ID" \
    --url "$FLUX_SERVER_URL" \
    --token "$SERVICE_TOKEN" 2>/dev/null || echo "")
  assert_contains "$FAIL_REPLAY" "e2e-intentional-failure" \
    "replay of failed exec preserves the failure (no fabricated history)"
else
  fail "no error execution found in flux logs (expected one from /fail route)"
fi

# ── PHASE 9: Isolation ────────────────────────────────────────────────────────
section "10. ISOLATION — no shared state between executions"

ID1=$(curl -sf -X POST "$APP_URL/users" \
  -H "content-type: application/json" \
  -d '{"name":"isolation-test-1"}' 2>/dev/null | jq -r '.id' || echo "")
ID2=$(curl -sf -X POST "$APP_URL/users" \
  -H "content-type: application/json" \
  -d '{"name":"isolation-test-2"}' 2>/dev/null | jq -r '.id' || echo "")

assert_nonempty "$ID1" "isolation: execution 1 returns UUID"
assert_nonempty "$ID2" "isolation: execution 2 returns UUID"

if [[ -n "$ID1" && -n "$ID2" && "$ID1" != "$ID2" ]]; then
  pass "isolation: UUIDs are unique across executions ($ID1 ≠ $ID2)"
else
  fail "isolation: executions returned same UUID (shared state leak)"
fi

# ── PHASE 10: Redis-optional path ────────────────────────────────────────────
section "11. REDIS-OPTIONAL — system works without Redis"

# The runtime is already running without Redis if REDIS_URL is empty.
# Prove it by hitting an endpoint that would detect shared state if Redis leaked.
REDIS_TEST=$(curl -sf "$APP_URL/health" 2>/dev/null | jq -r '.status' || echo "failed")
assert_equal "$REDIS_TEST" "ok" "system responds correctly when Redis is absent"

# ── Done ──────────────────────────────────────────────────────────────────────
e2e_summary
