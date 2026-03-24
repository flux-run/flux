#!/usr/bin/env bash
set -e

# A 1-Command Skateboard MVP Dev Environment
# Boots: flux-server, flux-runtime, flux-executor in parallel.

echo "=================================================="
echo "🚀 Booting Flux E2E Architecture (Skateboard MVP)"
echo "=================================================="

# Export minimal config needed for local dev
export DATABASE_URL=${DATABASE_URL:-"postgresql://flux:password@localhost:5432/flux"}
export INTERNAL_SERVICE_TOKEN="local-development-token"
export FLUX_SERVER_URL="http://127.0.0.1:50051"
export GRPC_PORT="50051"
export RUST_LOG="info"

# Ensure the DB is ready (Optional, assumes DB exists locally)
# make migrate || echo "Make sure postgres is running locally!"

echo "📦 1. Starting flux-server (Control/Data plane storage) on :50051"
SQLX_OFFLINE=true cargo run -p server > server.log 2>&1 &
SERVER_PID=$!

# Wait for server to bind
sleep 2

echo "⚙️ 2. Starting flux-runtime (Execution + Telemetry) on :8081"
export FLUX_SERVICE_TOKEN=$INTERNAL_SERVICE_TOKEN
cargo run -p runtime -- --serve --port 8081 --entry artifacts/create-order.js > runtime.log 2>&1 &
RUNTIME_PID=$!

# Wait for runtime to bind
sleep 2

echo "🌐 3. Starting flux-executor (Stateless HTTP Edge) on :8080"
cargo run -p flux-executor > executor.log 2>&1 &
EXECUTOR_PID=$!

echo "=================================================="
echo "✅ All systems go!"
echo "-> Executor listening on http://127.0.0.1:8080"
echo "-> Runtime listening on  http://127.0.0.1:8081"
echo "-> Server listening on   http://127.0.0.1:50051"
echo ""
echo "Press Ctrl+C to shutdown all services."
echo "Tail the logs with: tail -f server.log runtime.log executor.log"
echo "=================================================="

cleanup() {
    echo ""
    echo "Shutting down..."
    kill $SERVER_PID $RUNTIME_PID $EXECUTOR_PID 2>/dev/null || true
    wait $SERVER_PID $RUNTIME_PID $EXECUTOR_PID 2>/dev/null || true
    echo "Cleaned up."
    exit 0
}

trap cleanup INT TERM

# Wait infinitely until signaled
wait $SERVER_PID $RUNTIME_PID $EXECUTOR_PID
