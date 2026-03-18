# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-03-15

### Added
- `flux server start` / `flux server restart` — start and manage the `flux-server` gRPC process
- `flux serve <entry>` — launch `flux-runtime` to serve a JS/TS entry file
- `flux exec <entry>` — one-off execution without a long-running runtime
- `flux init` — interactive first-time setup (server URL + service token)
- `flux auth` — non-interactive auth and config save
- `flux config set/get` — inspect and update saved CLI config
- `flux logs` — list recorded executions with `--status`, `--path`, `--since`, `--search` filters
- `flux tail` — stream live execution events
- `flux trace <id>` — full execution trace with request/response and checkpoints
- `flux why <id>` — root-cause diagnosis for failed or slow executions
- `flux replay <id>` — replay an execution using recorded checkpoints (`--diff` for field comparison)
- `flux resume <id>` — resume an execution from a checkpoint boundary
- `flux ps` — show running `flux-server` and `flux-runtime` processes
- `flux status` — health summary of server, runtime, and Postgres
- `flux-server` — gRPC server backed by Postgres; stores executions, traces, and checkpoints
- `flux-runtime` — Deno V8 isolate executor; runs user JS/TS and records checkpoint spans
- Shared protobuf contract (`shared/proto/internal_auth.proto`) for all inter-process communication
- Runtime test suite (`runtime/tests/`) covering ECMAScript, Node.js APIs, Web APIs, determinism, error handling, and concurrency

[Unreleased]: https://github.com/flux-run/flux/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/flux-run/flux/releases/tag/v0.1.0
