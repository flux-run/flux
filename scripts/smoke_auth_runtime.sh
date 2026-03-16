#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${FLUX_SMOKE_PORT:-50051}"
TOKEN="${FLUX_SMOKE_TOKEN:-flux_smoke_token_local}"
URL="127.0.0.1:${PORT}"
DATABASE_URL_INPUT="${DATABASE_URL:-}"

if [[ -z "${DATABASE_URL_INPUT}" ]]; then
  echo "[fail] DATABASE_URL is required"
  echo "       Example: DATABASE_URL=postgres://localhost/flux make smoke-auth-runtime"
  exit 1
fi

if command -v pg_isready >/dev/null 2>&1; then
  if ! pg_isready -d "${DATABASE_URL_INPUT}" -t 2 >/dev/null 2>&1; then
    echo "[fail] PostgreSQL is not ready for DATABASE_URL=${DATABASE_URL_INPUT}"
    echo "       Start Postgres and apply required schema before running this smoke test."
    exit 1
  fi
elif command -v python3 >/dev/null 2>&1; then
  DB_HOST_PORT="$(python3 - <<'PY' "${DATABASE_URL_INPUT}"
import sys
from urllib.parse import urlparse

u = urlparse(sys.argv[1])
host = u.hostname or "localhost"
port = u.port or 5432
print(f"{host}:{port}")
PY
)"

  DB_HOST="${DB_HOST_PORT%:*}"
  DB_PORT="${DB_HOST_PORT##*:}"
  if ! nc -z "${DB_HOST}" "${DB_PORT}" >/dev/null 2>&1; then
    echo "[fail] PostgreSQL is not listening on ${DB_HOST}:${DB_PORT}"
    echo "       DATABASE_URL=${DATABASE_URL_INPUT}"
    exit 1
  fi
fi

TMP_DIR="$(mktemp -d)"
SERVER_LOG="${TMP_DIR}/server.log"
GOOD_AUTH_OUT="${TMP_DIR}/good_auth.out"
GOOD_LOGS_OUT="${TMP_DIR}/good_logs.out"
BAD_LOGS_OUT="${TMP_DIR}/bad_logs.out"
SERVE_OUT="${TMP_DIR}/serve.out"
ENTRY_FILE="${TMP_DIR}/hello.js"
SERVER_PID=""

cleanup() {
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT INT TERM

pass() {
  echo "[pass] $1"
}

fail() {
  echo "[fail] $1"
  echo
  if [[ -f "${SERVER_LOG}" ]]; then
    echo "--- server log tail ---"
    tail -n 60 "${SERVER_LOG}" || true
    echo "--- end server log tail ---"
  fi
  exit 1
}

cat >"${ENTRY_FILE}" <<'EOF'
export default async function hello() {
  return { ok: true };
}
EOF

cd "${ROOT_DIR}"

echo "[info] Building server + cli"
cargo build -p server -p cli >/dev/null

echo "[info] Starting server on ${URL}"
GRPC_PORT="${PORT}" \
DATABASE_URL="${DATABASE_URL_INPUT}" \
INTERNAL_SERVICE_TOKEN="${TOKEN}" \
cargo run -p server >"${SERVER_LOG}" 2>&1 &
SERVER_PID="$!"

for _ in {1..80}; do
  if nc -z 127.0.0.1 "${PORT}" >/dev/null 2>&1; then
    break
  fi

  if ! kill -0 "${SERVER_PID}" 2>/dev/null; then
    fail "server exited before becoming ready"
  fi

  sleep 0.25
done

if ! nc -z 127.0.0.1 "${PORT}" >/dev/null 2>&1; then
  fail "server did not start listening on ${URL}"
fi
pass "server started"

echo "[info] Verifying valid token"
if cargo run -p cli -- auth --url "${URL}" --token "${TOKEN}" >"${GOOD_AUTH_OUT}" 2>&1; then
  pass "auth accepts valid token"
else
  cat "${GOOD_AUTH_OUT}" || true
  fail "auth command failed with valid token"
fi

echo "[info] Saving config values"
cargo run -p cli -- config set token "${TOKEN}" >/dev/null 2>&1 || fail "failed to save token config"
cargo run -p cli -- config set server "${URL}" >/dev/null 2>&1 || fail "failed to save server config"
pass "config set token/server"

echo "[info] Verifying logs with good token"
if cargo run -p cli -- logs >"${GOOD_LOGS_OUT}" 2>&1; then
  pass "logs command works with valid auth"
else
  cat "${GOOD_LOGS_OUT}" || true
  fail "logs command failed with valid token"
fi

echo "[info] Verifying invalid token"
set +e
cargo run -p cli -- logs --token "bad_token_for_smoke" --url "${URL}" >"${BAD_LOGS_OUT}" 2>&1
BAD_RC=$?
set -e

if [[ "${BAD_RC}" -eq 0 ]]; then
  cat "${BAD_LOGS_OUT}" || true
  fail "bad token unexpectedly accepted"
fi

if grep -Eqi "unauthenticated|rejected|invalid service token" "${BAD_LOGS_OUT}"; then
  pass "bad token rejected"
else
  cat "${BAD_LOGS_OUT}" || true
  fail "bad token failed, but not with expected auth error"
fi

echo "[info] Verifying runtime serve handshake"
if cargo run -p cli -- serve "${ENTRY_FILE}" --url "${URL}" --token "${TOKEN}" >"${SERVE_OUT}" 2>&1; then
  if grep -q "runtime artifact prepared" "${SERVE_OUT}"; then
    pass "serve command validates token and prepares artifact"
  else
    cat "${SERVE_OUT}" || true
    fail "serve command succeeded without expected output"
  fi
else
  cat "${SERVE_OUT}" || true
  fail "serve command failed"
fi

echo
pass "smoke test completed"
echo "[note] logs/curl execution-record checks are pending implementation of server execution RPC + flux logs command"
