# Gateway

The gateway is the ingress layer for Flux.

It is responsible for turning an incoming trigger into a controlled execution with a stable request identity and consistent policy enforcement.

## Responsibilities

The gateway owns:

- route resolution
- auth and API key handling
- schema validation
- request middleware
- rate limiting
- request IDs and top-level trace metadata
- forwarding into the runtime

If a request bypasses the gateway, the debugging and policy story becomes weaker.

## Why The Gateway Matters To The Product

Flux is not trying to be only a function executor. It is trying to own enough of the request path that an operator can trust the execution record.

The gateway matters because it ensures that:

- every request starts with a trace root
- every request has a consistent identity
- auth, validation, and middleware happen inside the same record
- request metadata is available to `trace`, `why`, replay, and diff

## Route Model

A Flux route declares:

- method and path
- target function
- auth rules
- rate limits
- middleware
- validation or schema requirements

The route model is not just an ingress table. It is part of how Flux explains backend behavior.

## Middleware Model

Middleware exists so cross-cutting concerns stay inside the same runtime:

- auth
- request enrichment
- permission checks
- request shaping
- audit logic

Middleware is visible in traces so operators can tell whether time or failures came from app code or infrastructure policy.

## Replay And Debugging

Because the gateway owns the first step of a request, it is also the right place to preserve:

- request metadata
- headers relevant to replay or operator tooling
- route-level context needed for explanation

This is why invoking through the gateway is the most representative local test path.

## Target Production Shape

In production:

- the gateway is externally exposed
- internal services stay private
- rate limiting and auth live here
- request identities originate here
- the execution record starts here

The gateway is the front door of the runtime, not a disposable proxy.
