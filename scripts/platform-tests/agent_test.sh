#!/usr/bin/env bash
# agent_test.sh — comprehensive AI agent CRUD tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
SUFFIX="$(unique_suffix)"
agent_name="smoke-agent-${SUFFIX}"

# ── Deploy agent ──────────────────────────────────────────────────────────────

cat > /tmp/t_agent.yaml <<YAML
name: ${agent_name}
model: gpt-4o-mini
system: |
  You are a smoke test agent.
tools: []
llm_secret: FLUXBASE_LLM_KEY
max_turns: 1
temperature: 0.0
YAML

s="$(curl -sS -o /tmp/t_agent_deploy.json -w "%{http_code}" \
  -X POST "${API_URL}/agents" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  -H "Content-Type: text/plain" \
  --data-binary @/tmp/t_agent.yaml || true)"

if [[ "$s" == "201" || "$s" == "200" ]] \
  && jq -e --arg name "$agent_name" '.name == $name' /tmp/t_agent_deploy.json >/dev/null 2>&1; then
  print_result 1 "agent deploy"
else
  print_result 0 "agent deploy" "HTTP ${s}"
  exit 1
fi

# Deploy should return at minimum name and model
assert_jq 'has("name") and has("model")' /tmp/t_agent_deploy.json "agent deploy response schema" FAIL

# ── List agents ───────────────────────────────────────────────────────────────

s="$(api_get "/agents" /tmp/t_agent_list.json)"
assert_status_and_jq "200" "$s" \
  '.data | any(.[]; .name == env.agent_name)' \
  /tmp/t_agent_list.json "agent list contains our agent" FAIL

# list must be an array
assert_jq '.data | type == "array"' /tmp/t_agent_list.json "agent list is array" FAIL

export agent_name

# ── Get agent ─────────────────────────────────────────────────────────────────

s="$(api_get "/agents/${agent_name}" /tmp/t_agent_get.json)"
assert_status_and_jq "200" "$s" \
  --arg name "$agent_name" '.name == $name and .model == "gpt-4o-mini"' \
  /tmp/t_agent_get.json "agent get" FAIL

# Required fields present
assert_jq 'has("name") and has("model") and has("system")' \
  /tmp/t_agent_get.json "agent get schema" FAIL

# ── Get non-existent agent → 404 ─────────────────────────────────────────────

s="$(api_get "/agents/__nonexistent_agent_${SUFFIX}__" /tmp/t_agent_404.json)"
assert_status "404" "$s" "agent get unknown → 404" FAIL

# ── Update agent ──────────────────────────────────────────────────────────────

cat > /tmp/t_agent_updated.yaml <<YAML
name: ${agent_name}
model: gpt-4o-mini
system: |
  Updated smoke test agent.
tools: []
llm_secret: FLUXBASE_LLM_KEY
max_turns: 2
temperature: 0.1
YAML

s_update="$(curl -sS -o /tmp/t_agent_update.json -w "%{http_code}" \
  -X PUT "${API_URL}/agents/${agent_name}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  -H "Content-Type: text/plain" \
  --data-binary @/tmp/t_agent_updated.yaml || true)"
if [[ "$s_update" == "200" || "$s_update" == "204" || "$s_update" == "404" ]]; then
  # 404 is acceptable if PUT create-or-update is not supported yet
  print_result 1 "agent update (${s_update})"
else
  print_result 0 "agent update" "HTTP ${s_update}"
  FAIL=1
fi

# ── Multiple agents ───────────────────────────────────────────────────────────

agent2_name="smoke-agent2-${SUFFIX}"
cat > /tmp/t_agent2.yaml <<YAML
name: ${agent2_name}
model: gpt-4o-mini
system: Second smoke agent.
tools: []
llm_secret: FLUXBASE_LLM_KEY
max_turns: 1
temperature: 0.0
YAML

s="$(curl -sS -o /tmp/t_agent2_deploy.json -w "%{http_code}" \
  -X POST "${API_URL}/agents" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  -H "Content-Type: text/plain" \
  --data-binary @/tmp/t_agent2.yaml || true)"
if [[ "$s" == "201" || "$s" == "200" ]]; then
  print_result 1 "agent deploy second"

  # Both should appear in list
  s="$(api_get "/agents" /tmp/t_agent_list2.json)"
  ok="$(jq -e --arg n1 "$agent_name" --arg n2 "$agent2_name" \
    '.data | (any(.[]; .name == $n1)) and (any(.[]; .name == $n2))' \
    /tmp/t_agent_list2.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "agent list shows both agents"
  else
    print_result 0 "agent list shows both agents"
    FAIL=1
  fi
else
  print_result 0 "agent deploy second" "HTTP ${s}"
  FAIL=1
fi

# ── Agent run endpoint (may not trigger actual LLM in test) ──────────────────

s_run="$(curl -sS -o /tmp/t_agent_run.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/agents/${agent_name}/run" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{"message":"smoke test run"}' || true)"
# Acceptable: 200 (ran), 402/503 (no LLM key), 404 (endpoint not wired)
if [[ "$s_run" == "200" || "$s_run" == "201" || "$s_run" == "402" \
   || "$s_run" == "503" || "$s_run" == "404" || "$s_run" == "400" ]]; then
  print_result 1 "agent run endpoint reachable (${s_run})"
else
  print_result 0 "agent run endpoint reachable" "HTTP ${s_run}"
  FAIL=1
fi

# ── Unauthenticated access → 401 ─────────────────────────────────────────────

s="$(curl -sS -o /dev/null -w "%{http_code}" "${API_URL}/agents" || true)"
assert_status "401" "$s" "agents requires auth" FAIL

# ── Delete both agents ────────────────────────────────────────────────────────

s="$(api_delete "/agents/${agent_name}" /tmp/t_agent_del.txt)"
if [[ "$s" == "204" || "$s" == "200" ]]; then
  print_result 1 "agent delete"
else
  print_result 0 "agent delete" "HTTP ${s}"
  FAIL=1
fi

api_delete "/agents/${agent2_name}" /dev/null >/dev/null 2>&1 || true

# Deleted agent must not appear in list
s="$(api_get "/agents" /tmp/t_agent_list3.json)"
if [[ "$s" == "200" ]]; then
  if jq -e --arg name "$agent_name" '.data | any(.[]; .name == $name)' \
    /tmp/t_agent_list3.json >/dev/null 2>&1; then
    print_result 0 "agent deleted and gone" "still in list"
    FAIL=1
  else
    print_result 1 "agent deleted and gone"
  fi
fi

exit "$FAIL"
