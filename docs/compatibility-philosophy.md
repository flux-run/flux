# Flux Compatibility Philosophy

Flux does not try to run everything. It guarantees correctness for what it runs.

The primary difference between a generic JavaScript runtime (like Node.js, Deno, or Bun) and the Flux execution engine is **deterministic execution guarantees**. A library or driver is supported ONLY if its I/O flows entirely through Flux-controlled boundaries. 

## The Three Tiers of Compatibility

### 1. Supported via Controlled Boundary (Green)
Libraries that use standard Web APIs or provide adapters that map directly to Flux's intercepted interfaces are fully supported out-of-the-box.
- `fetch` (native API)
- `flux:pg` (drop-in, node-postgres compatible)
- `flux:redis` (Node Redis compatible)

### 2. Adapted Compatibility (Yellow)
Libraries that include features incompatible with determinism (like pub/sub, background reconnects, or dynamic pipelines) but have a salvageable core API. These are supported via thin integration adapters that:
- Preserve API familiarity for supported operations.
- Explicitly block or throw on unsupported features, ensuring they **fail loudly** rather than behaving non-deterministically.
- *Example:* The `ioredis` adapter wraps `flux:redis` but explicitly blocks `multi()` and `subscribe()`.

### 3. Explicitly Rejected (Red)
Libraries that rely on raw TCP (`net.Socket`), custom wire protocols, or unmanaged background connection pooling bypass the Flux execution boundary. 
We do not try to "hack" these to work. They are fundamentally architecturally incompatible with execution replays and determinism, and are thus **explicitly rejected** by the system.
- *Example:* `postgres.js` requires raw TCP access and is therefore blocked at the boundary.

## Contract vs. Tooling
When integrating with the ecosystem, the question is not *"does it run?"* but rather *"does it obey the execution contract?"*

Any driver or library that attempts to circumvent the sandbox's determinism must fail with a strong, machine-verifiable, structured error. For example, unsupported drivers yield:

```json
{
  "category": "unsupported-driver",
  "reason": "raw TCP bypasses Flux execution boundary",
  "recommendation": "use flux:pg"
}
```

By defining the boundary of correctness explicitly, Flux prioritizes system guarantees over raw compatibility stats.
