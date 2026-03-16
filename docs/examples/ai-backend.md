# Example: AI Backend

This example shows how Flux can run an AI-backed backend without losing operational clarity.

The point is not "AI features." The point is that prompts, tool calls, database writes, and background work can still live inside one execution model.

## What The Example Covers

- an HTTP endpoint for user requests
- retrieval or database-backed context loading
- function execution and external tool calls
- queue-backed follow-up work
- mutation recording for AI-driven actions
- traces and explanation for AI-driven flows

## Why This Example Matters

AI systems increase debugging difficulty:

- prompts change
- tools fail
- external APIs are slow
- state can be mutated indirectly

Flux preserves the same operator clarity here that it offers for normal backends.

## Good Demo Flow

```bash
flux init ai-backend
flux dev
flux invoke ask_support --payload '{"message":"refund my order"}'
flux trace
flux why <request_id>
flux incident replay --request-id <request_id>
```

## What A Reader Should Learn

This example shows:

- why AI functions belong inside the same runtime
- how external tool calls become part of traces
- how stateful AI actions can still be audited and debugged
