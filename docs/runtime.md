# Runtime

The runtime executes user code.

It is the component that loads a function bundle, provides the execution context, enforces limits, and turns a routed request or background trigger into a result.

## Responsibilities

The runtime owns:

- bundle lookup and loading
- execution of JavaScript or WebAssembly handlers
- limit enforcement such as timeout and memory policies
- secret and configuration access during execution
- span and log emission
- integration with database dispatch and queue surfaces

The runtime is not the primary place for ingress policy or broad operator APIs.

## Execution Lifecycle

A typical runtime execution looks like this:

1. receive an execution request from runtime request handling, queue, or schedule
2. resolve the function version and load the bundle
3. construct the Flux execution context
4. execute the handler under configured limits
5. capture spans, logs, errors, and output
6. connect the result back to the execution record

The product depends on that lifecycle being observable, not just fast.

## Bundle Model

Flux uses deployable function bundles.

The runtime:

- loads the correct version
- caches hot bundles
- invalidates caches after deployment
- executes the same code shape locally and in production

Bundle identity matters because debugging almost always depends on knowing exactly what code ran.

## Runtime Context

The execution context gives handlers access to:

- request metadata
- secrets
- database access
- queue publishing
- tracing hooks
- deployment and environment metadata

This context is how Flux turns a collection of services into one programming model.

## Limits And Safety

The runtime is also where the platform enforces:

- timeout limits
- memory ceilings
- language/runtime-specific guardrails
- controlled side-effect surfaces

The goal is predictable execution that stays explainable.

## Why The Runtime Matters To Flux

The runtime is important because it is where code, state, and observability meet.

If Flux cannot say:

- which bundle ran
- under what limits
- with what inputs
- producing which spans and side effects

then the rest of the product story collapses.
