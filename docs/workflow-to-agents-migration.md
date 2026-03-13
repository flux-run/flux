# Migration: Workflows & Tools → Agents

**Date:** 2025-01-XX
**Impact:** Breaking — removes two primitives, adds one

---

## What changed

Flux had 6 primitives: Function, Database, Queue, Agent, Workflow, Tool.
Now it has 4: **Function, Database, Queue, Agent**.

| Removed | Replacement |
|---|---|
| `defineWorkflow()` | Agent with ordered tool calls, or just chain functions manually |
| `ctx.workflow.start()` | `ctx.agent.run()` |
| `ctx.tools.*` | Functions ARE tools — agents reference them by name |
| `flux add <tool>` CLI | No separate tool registry — write a function that wraps the SDK |
| `tools/` directory | Gone — third-party integrations are just functions in `functions/` |
| `workflows/` directory | Replaced by `agents/` |

---

## Why

- **Workflows competed with agents.** Both orchestrated multi-step execution. Having both confused the mental model.
- **Tools were unnecessary indirection.** A "Stripe tool" is just a function that calls the Stripe SDK. No registry needed.
- **4 primitives is the right number.** Function (compute), Database (state), Queue (async), Agent (intelligence). Everything else composes from these.

---

## New primitive: Agent

```ts
// agents/support-router.ts
import { defineAgent } from "@flux/functions";

export default defineAgent({
  name: "support-router",
  model: "gpt-4o",
  instructions: "You are a support agent. Classify tickets and route them.",
  tools: ["classify_ticket", "assign_to_team", "send_notification"],
});
```

- `tools` references function names — those functions must exist in `functions/`
- Each tool call is recorded as an `agent_step` span in the ExecutionRecord
- Third-party integrations (Stripe, Twilio, etc.) are just functions that wrap the SDK

### FluxContext changes

```ts
// Before
ctx.workflow.start("onboarding", { userId });
ctx.tools.stripe.createCustomer({ email });

// After
ctx.agent.run("support-router", { ticket });
// For Stripe: just call a function
ctx.fn("create_stripe_customer", { email });
```

---

## Code changes required

### Delete

| What | Where |
|---|---|
| Workflow engine | `data-engine/src/workflows/` (if exists) |
| Workflow routes | API routes for `/workflows/*` |
| Tool registry | `data-engine/src/tools/` or `api/src/tools/` (if exists) |
| Tool routes | API routes for `/tools/*` |
| `defineWorkflow` export | `packages/sdk/` or `@flux/functions` |
| `ctx.workflow` | Runtime's FluxContext builder |
| `ctx.tools` | Runtime's FluxContext builder |
| `tool_call` span kind | ExecutionRecord span type enum |
| Workflow CLI commands | `cli/src/` — `flux workflow *` commands |
| Tool CLI commands | `cli/src/` — `flux tool *`, `flux add <tool>` commands |
| `workflows/` in project scan | CLI's directory scanner for deploy/build |
| `tools/` in project scan | CLI's directory scanner for deploy/build |

### Add

| What | Where |
|---|---|
| `defineAgent()` | `packages/sdk/src/agent.ts` — export from `@flux/functions` |
| `ctx.agent.run()` | Runtime's FluxContext builder — dispatches to agent runtime |
| `agent_step` span kind | ExecutionRecord span type enum |
| Agent runtime | `runtime/src/agents/` — receives agent definition, calls LLM, dispatches tool calls as function invocations, records each step |
| Agent routes | API: `POST /flux/api/agents/invoke`, `GET /flux/api/agents`, `GET /flux/api/agents/:name` |
| Agent CLI commands | `cli/src/` — `flux agent list`, `flux agent invoke <name>`, `flux agent logs <name>` |
| `agents/` directory scan | CLI's directory scanner recognizes `agents/*.ts` |
| Agent deploy | `flux deploy` scans `agents/` alongside `functions/` |

### Modify

| What | Change |
|---|---|
| `ExternalCall.kind` enum | Remove `"tool"`, add `"agent_step"` |
| `ExecutionSpan` types | Add `agent_step` variant with `model`, `tokens_used`, `tool_calls` |
| Project structure docs | `workflows/` → `agents/`, remove `tools/` |
| `flux.toml` | Remove `[workflows]` and `[tools]` sections, add `[agents]` |
| Dashboard UI | Replace workflow viewer with agent trace viewer (shows LLM reasoning + tool call sequence) |

---

## Agent ExecutionRecord shape

When an agent runs, the record looks like:

```
ExecutionRecord
├── fn.support-router          (agent entry point)
│   ├── agent_step             (LLM call #1 — decides to classify)
│   │   └── fn.classify_ticket (function invoked as tool)
│   ├── agent_step             (LLM call #2 — decides to assign)
│   │   └── fn.assign_to_team  (function invoked as tool)
│   └── agent_step             (LLM call #3 — decides to notify)
│       └── fn.send_notification
```

Each `agent_step` span records:
- `model` — which LLM was called
- `prompt_tokens` / `completion_tokens` — token usage
- `tool_choice` — which function the LLM chose to call
- `reasoning` — the LLM's output before tool selection (if available)

This means `flux trace` and `flux why` work for agent executions exactly like they work for functions — you see every decision the agent made and every function it called.

---

## Migration order

1. **SDK first** — add `defineAgent()` export, add `ctx.agent.run()` to FluxContext type definitions
2. **Runtime** — build agent execution loop (LLM call → function dispatch → record → repeat)
3. **API routes** — add agent CRUD + invoke endpoints
4. **CLI** — add `flux agent *` commands, remove `flux workflow *` and `flux tool *`
5. **Dashboard** — agent trace viewer
6. **Delete** — remove all workflow/tool code after agent is working

---

## Key design decisions

- **Agents are NOT functions.** They live in `agents/`, not `functions/`. They have a different execution model (loop vs single-shot).
- **Functions ARE tools.** No separate tool concept. An agent's `tools` array references function names.
- **Third-party = functions.** `create_stripe_customer` is a function in `functions/` that imports the Stripe SDK. No tool registry, no plugin system.
- **Recording is automatic.** Every LLM call, every tool dispatch, every token count — all captured in the ExecutionRecord without developer instrumentation.
