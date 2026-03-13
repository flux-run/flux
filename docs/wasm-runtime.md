# WASM Runtime

Flux is designed to support both JavaScript-first execution and a broader WebAssembly story.

The WASM path matters because it opens the door to multi-language functions while preserving a consistent runtime model.

## Why WASM Matters

WebAssembly gives Flux a path to:

- language diversity
- portable function bundles
- tighter runtime control
- a more uniform deployment artifact for non-JavaScript languages

That matters for the product because Flux wants one runtime, not a different execution model for every language.

## Product Stance

The JavaScript path can be the first-class default while WASM matures.

The goal is not immediate parity across every language. The goal is:

- a coherent packaging model
- predictable execution
- visibility into spans, errors, and side effects
- deployment metadata that still fits the same debugging story

## What Parity Means

WASM support is successful when a non-JavaScript function still participates in:

- deployment versioning
- trace generation
- mutation attribution
- queue and schedule execution
- replay and diff, where practical

Without that, WASM is just an alternate build target, not part of the product.

## Packaging Model

The target model is:

- source language compiles to a WASM artifact
- Flux stores that artifact as a function version
- the runtime loads and executes it under the same execution record model

The packaging process may differ by language, but the operator experience stays consistent.

## Constraints

WASM support comes with realistic constraints:

- some language features may require adapters or tooling
- I/O and host bindings need a stable contract
- performance and startup characteristics may differ by language

Those constraints are acceptable as long as the runtime story stays coherent.

## Product Rule

WASM is valuable when it strengthens the complete-system story.

It is not valuable if it adds language breadth while weakening the debugging and deployment model.
