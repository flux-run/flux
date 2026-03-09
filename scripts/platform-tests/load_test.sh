#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

payload='{"table":"users","operation":"select","limit":1}'

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

results="$(seq 1 50 | xargs -I{} -P 20 bash -lc 'run_one')"
printf '%s\n' "$results" > /tmp/platform_load_results.txt

non_200="$(awk '$1 != 200 {c++} END{print c+0}' /tmp/platform_load_results.txt)"
hits="$(awk '$2 == "HIT" {c++} END{print c+0}' /tmp/platform_load_results.txt)"
avg_ms="$(awk '{sum += $3} END {if (NR>0) printf "%.2f", (sum/NR)*1000; else print "0"}' /tmp/platform_load_results.txt)"

if [[ "$non_200" == "0" ]]; then
  print_result 1 "concurrency"
else
  print_result 0 "concurrency" "non_200=${non_200}"
  exit 1
fi

echo "[INFO] load summary (avg_ms=${avg_ms}, cache_hits=${hits}/50)"
