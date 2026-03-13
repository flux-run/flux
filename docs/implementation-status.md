# Implementation Status

This page keeps the rest of the docs honest.

Most of the documentation in this repo describes the intended 0.1 beta product shape. This document explains where the codebase is still converging toward that shape.

## Status Levels

- `Active` - the direction is correct and the core implementation exists
- `Shaping` - the product surface is clear but the implementation is still being aligned
- `Experimental` - useful for exploration, not yet a stable promise
- `Needs hardening` - the feature exists, but the operational contract is not ready

## Current Snapshot

| Area | Status | Notes |
| --- | --- | --- |
| Product narrative | Active | The strongest product direction is clear: complete runtime, debug-first story. |
| Documentation | Active | The docs now describe the intended product shape and should be the canonical narrative. |
| Single-binary direction | Active | The `server` crate is the right architectural direction even though individual crates still exist and are still used during development. |
| CLI core loop | Shaping | The right commands exist, but command naming, config resolution, and contract cleanup still need work. |
| Local dev story | Active | `flux dev` is the right idea and one of the most important product surfaces. |
| Gateway | Active | The request pipeline is strong and aligned with the product model. |
| Runtime | Active | Execution, bundle loading, and caching are substantive. Some endpoint and auth surfaces still need cleanup. |
| Data engine | Active | Mutation-aware execution is a core strength of the repo. |
| Queue and schedules | Shaping | Important to the complete-system story, but still need smoother execution-record integration and operator polish. |
| Replay and diff | Shaping | High value, but trustworthiness matters more than breadth here. |
| Agents | Experimental | Useful as part of the system, but not yet the headline feature. |
| WASM and multi-language parity | Experimental | Ambitious and worth keeping, but not yet a dependable flagship capability. |
| Auth and service hardening | Needs hardening | Safe defaults and service isolation need more work before broad beta testing. |

## What Must Feel Excellent Before 0.1 Beta

These are the gates that matter most:

1. `flux init -> flux dev -> flux invoke -> flux trace -> flux why` feels clean
2. project and config resolution are easy to understand
3. one deployment is visibly linked to one execution record
4. one replay-plus-diff flow is believable enough to trust
5. async work preserves the same debugging model
6. defaults are safe enough for real beta users

## How To Use The Docs

Use the rest of the docs for:

- product intent
- architecture
- desired workflows

Use this page for:

- implementation caveats
- maturity expectations
- deciding which areas are ready for hard external testing
