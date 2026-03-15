# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-03-15

### Added
- `flux dev` — single-command local development server with embedded PostgreSQL
- `flux deploy` — deploy functions to production
- `flux init` — scaffold a new Flux project
- `flux new` — create a new function
- `flux invoke` — invoke a function locally or in production
- `flux trace <id>` — full distributed trace as a waterfall
- `flux why <id>` — root-cause analysis in one command
- `flux tail` — live request stream with optional `--errors` filter
- `flux state history` / `flux state blame` — full row version history
- `flux incident replay <id>` — replay a request with recorded state
- `flux trace diff <a> <b>` — compare two executions field-by-field
- `flux bug bisect` — binary-search commits to find a regression
- Gateway service with in-memory route snapshot and Postgres LISTEN/NOTIFY
- Runtime service with Deno V8 isolates for TypeScript/JavaScript functions
- Data Engine with atomic mutation recording and cron support
- Queue service with DB-backed job polling, retries, and dead-letter queue
- API service for function registry, secrets, and schema management
- `server` monolith binary combining all five services
- WebAssembly function support (Go, Rust, AssemblyScript, C, C++)
- AES-256-GCM encrypted secrets with LRU cache
- Anonymous CLI telemetry (opt-out: `FLUX_NO_TELEMETRY=1`)

[Unreleased]: https://github.com/flux-run/flux/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/flux-run/flux/releases/tag/v0.1.0
