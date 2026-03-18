# Redis Boundary Contract

This document defines the Redis v1 contract in Flux.

Redis in Flux is a deterministic command boundary. It is not a Redis client, SDK, or socket escape hatch.

## Core Rule

Flux owns Redis side effects.

User code may use a compatibility surface like `import { createClient } from "redis"`, but execution always flows through the Flux runtime boundary:

- user code calls a Redis-like API
- the shim translates the call into a command envelope
- `op_flux_redis_command` executes the command in Rust
- Flux records the request and response as a checkpoint
- replay returns the recorded response without touching Redis

That means Redis follows the same product model as other Flux-owned boundaries:

- `fetch` is an HTTP boundary
- `postgres` is a SQL boundary
- `redis` is a command boundary

## Supported Surface

The compatibility shim intentionally exposes a small allowlist of high-value commands.

### String and key commands

- `client.get(key)`
- `client.set(key, value)`
- `client.del(...keys)`
- `client.exists(key)`

### Numeric commands

- `client.incr(key)`
- `client.decr(key)`

### Expiry commands

- `client.expire(key, seconds)`
- `client.ttl(key)`

### Hash commands

- `client.hGet(key, field)`
- `client.hSet(key, field, value)`
- `client.hDel(key, field)`

### Escape hatch

- `client.sendCommand(args)`
- `Flux.redis.command({ connectionString, command, args })`

The escape hatch still goes through the same Flux-controlled boundary. It is intended for power users and test scenarios, not for recreating a full Redis client in JavaScript.

## Compatibility No-Ops

These methods exist only so common Redis client code can initialize cleanly inside Flux:

- `client.connect()`
- `client.disconnect()`
- `client.quit()`

They do not create or manage a long-lived socket. They resolve immediately and do not create replay-visible client state.

## Explicitly Unsupported

The following features are outside the v1 contract and must fail loudly.

### Transactions

- `multi`
- `exec`
- `watch`
- `unwatch`

### Pub/Sub

- `subscribe`
- `psubscribe`
- `publish`
- `unsubscribe`

### Pipelines

- `pipeline`
- `batch`

### Blocking commands

- `BLPOP`
- `BRPOP`
- `XREAD BLOCK`

Flux rejects these because they depend on session state, long-lived socket behavior, or non-deterministic timing semantics that do not fit the execution model.

Errors should follow this shape:

```text
Redis <feature> is not supported in Flux (non-deterministic execution)
```

## Response Shape

Redis responses returned to JavaScript must be JSON-safe and replay-safe.

Current normalization rules:

- bulk string -> `string`
- nil -> `null`
- integer -> `number`
- array -> JSON array
- simple string -> `string`

Flux does not expose raw RESP frames or `Buffer` values at this boundary.

## Replay Rules

Replay never touches Redis.

On live execution:

- Flux sends the command to Redis
- Flux records the response checkpoint

On replay:

- Flux returns the recorded response
- Flux does not re-execute the command

Example:

```ts
await client.incr("counter")
```

Live execution may mutate Redis and return `1`.

Replay returns the recorded value `1` again. It does not increment Redis to `2`.

The same rule applies to mutating commands like `SET`, `DEL`, `HSET`, and `EXPIRE`: replay returns the recorded result without reapplying the mutation.

## Safety Rules

Redis uses the same outbound safety model as other Flux boundaries:

- restricted hosts are blocked
- private and loopback targets are blocked by default
- loopback can be allowed explicitly for local development or tests

Current override:

- `FLOWBASE_ALLOW_LOOPBACK_REDIS=1`

## No Hidden JavaScript State

The Redis shim must not accumulate hidden execution state in JavaScript.

That means:

- no JS connection pooling
- no JS caching layer
- no transaction/session state
- no JS-owned replay shortcuts

Everything that matters for correctness must cross the Rust boundary and be visible as a checkpoint.

## Trace Semantics

Redis checkpoints should be legible in `flux trace`.

Examples:

```text
REDIS GET "user:1" -> "shashi"
REDIS INCR "counter" -> 42
```

Trace should explain the command and the returned value directly. Redis is shared state, not an optimization cache, so cache-oriented language should not be attached to this boundary.

## Product Positioning

Flux is not building a Redis client.

Flux is building a deterministic, replayable shared-state boundary that currently uses Redis as the backend implementation.

That distinction preserves the architecture:

- runtime owns side effects
- checkpoints own replay truth
- JavaScript gets a thin compatibility illusion, not transport control