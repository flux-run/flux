#!/usr/bin/env bash
# load_test.sh — load and concurrency tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
payload='{"table":"users","operation":"select","limit":1}'

# ── Helper for a single request (used by xargs) ───────────────────────────────

run_one() {
  curl -sS -o /dev/null -D - -w "HTTP:%{http_code} TIME:%{time_total}\n" \
    -X POST "${GATEWAY_URL}/db/query" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
    -H "X-Fluxbase-Project: ${PROJECT_ID}" \
    --data "${payload}" | awk 'BEGIN{code="";cache="";time=""} /^x-cache:/ {cache=toupper($2)} /^HTTP:/ {split($1,a,":"); code=a[2]; split($2,b,":"); time=b[2]} END{gsub(/\r/,"",cache); print code, cache, time}'
}

export -f run_one
export GATEWAY_URL TOKEN TENANT_ID PROJECT_ID payload

# ── Concurrency test: 50 parallel DB queries ──────────────────────────────────

results="$(seq 1 50 | xargs -I{} -P 20 bash -lc 'run_one' 2>/dev/null || true)"
printf '%s\n' "$results" > /tmp/t_load_50.txt

non_200="$(awk '$1 != 200 {c++} END{print c+0}' /tmp/t_load_50.txt)"
hits="$(awk '$2 == "HIT" {c++} END{print c+0}' /tmp/t_load_50.txt)"
avg_ms="$(awk '{sum += $3} END {if (NR>0) printf "%.0f", (sum/NR)*1000; else print "0"}' /tmp/t_load_50.txt)"
max_ms="$(awk '{v=$3*1000; if(v>m)m=v} END{printf "%.0f", m+0}' /tmp/t_load_50.txt)"
p95_ms="$(awk '{print $3*1000}' /tmp/t_load_50.txt | sort -n | awk 'BEGIN{n=0}{a[n++]=$1}END{p=int(n*0.95); printf "%.0f", a[p]+0}')"

if [[ "$non_200" == "0" ]]; then
  print_result 1 "load 50 concurrent DB queries all 200"
else
  print_result 0 "load 50 concurrent DB queries" "non_200=${non_200}/50"
  FAIL=1
fi

echo "[INFO] db load: avg=${avg_ms}ms p95=${p95_ms}ms max=${max_ms}ms cache_hits=${hits}/50"

# p95 must be under 2 seconds for in-memory path
if python3 -c "import sys; sys.exit(0 if int('${p95_ms}') < 2000 else 1)" 2>/dev/null; then
  print_result 1 "load p95 < 2000ms (${p95_ms}ms)"
else
  print_result 0 "load p95 < 2000ms" "p95=${p95_ms}ms"
  FAIL=1
fi

# ── Function invocation load: 20 parallel ─────────────────────────────────────

fn_invoke_one() {
  curl -sS -o /dev/null -w "%{http_code}" \
    -X POST "${FN_LOAD_GW}/${FN_LOAD_NAME}" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${FN_LOAD_TOKEN}" \
    -H "X-Fluxbase-Tenant: ${FN_LOAD_TENANT}" \
    -H "X-Fluxbase-Project: ${FN_LOAD_PROJECT}" \
    --data '{"message":"load"}' || true
}
export -f fn_invoke_one
export FN_LOAD_GW="$GATEWAY_URL"
export FN_LOAD_NAME="$FUNCTION_NAME"
export FN_LOAD_TOKEN="$TOKEN"
export FN_LOAD_TENANT="$TENANT_ID"
export FN_LOAD_PROJECT="$PROJECT_ID"

fn_results="$(seq 1 20 | xargs -I{} -P 20 bash -lc 'fn_invoke_one' 2>/dev/null || true)"
fn_non_200="$(printf '%s\n' "$fn_results" | grep -cv '^200$' || true)"
if [[ "$fn_non_200" == "0" ]]; then
  print_result 1 "load 20 concurrent function invocations all 200"
else
  print_result 0 "load 20 concurrent function invocations" "non_200=${fn_non_200}/20"
  FAIL=1
fi

# ── Ramp test: 10 → 30 sequential bursts ─────────────────────────────────────

total_errors=0
for concurrency in 10 20 30; do
  burst_results="$(seq 1 "$concurrency" | xargs -I{} -P "$concurrency" bash -lc 'fn_invoke_one' 2>/dev/null || true)"
  burst_non_200="$(printf '%s\n' "$burst_results" | grep -cv '^200$' || true)"
  total_errors=$((total_errors + burst_non_200))
  echo "[INFO] ramp c=${concurrency}: non_200=${burst_non_200}"
done

if [[ "$total_errors" -eq 0 ]]; then
  print_result 1 "load ramp test (10→20→30) zero errors"
else
  print_result 0 "load ramp test" "total_errors=${total_errors}"
  FAIL=1
fi

# ── API health under load ─────────────────────────────────────────────────────

health_load_one() {
  curl -sS -o /dev/null -w "%{http_code}" "${HEALTH_LOAD_URL}/health" || true
}
export -f health_load_one
export HEALTH_LOAD_URL="$API_URL"

health_results="$(seq 1 30 | xargs -I{} -P 30 bash -lc 'health_load_one' 2>/dev/null || true)"
health_non_200="$(printf '%s\n' "$health_results" | grep -cv '^200$' || true)"
if [[ "$health_non_200" == "0" ]]; then
  print_result 1 "load 30 concurrent /health all 200"
else
  print_result 0 "load 30 concurrent /health" "non_200=${health_non_200}/30"
  FAIL=1
fi

exit "$FAIL"
