# Concepts

## Core Model

Flux treats each function run as an execution record.

Each record ties together:

- input payload
- output/error
- checkpointed IO calls
- duration
- code/version identity

## Why This Matters

Debugging starts from one execution ID instead of stitching logs, traces, and DB state from different tools.

## Checkpoints

Checkpointed boundaries (for replay/resume) capture request/response and timing for side-effect calls.

This enables:

- deterministic replay (`flux replay`)
- partial continuation (`flux resume`)
- field-level output comparison (`flux replay --diff`)

## Operator Loop

1. find issue (`flux logs --status error`)
2. inspect full context (`flux trace <id> --verbose`)
3. diagnose quickly (`flux why <id>`)
4. validate fix behavior (`flux replay <id> --diff`)
