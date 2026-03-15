# Contributing to Flux

Thank you for your interest in contributing! This guide covers everything you need to get the project running locally and submit a pull request.

## Table of contents

- [Project structure](#project-structure)
- [Prerequisites](#prerequisites)
- [Running locally](#running-locally)
- [Running tests](#running-tests)
- [Submitting a pull request](#submitting-a-pull-request)
- [Coding conventions](#coding-conventions)

---

## Project structure

```
Flux is a mixed Rust + TypeScript monorepo:

Rust workspace (Cargo.toml):
  api/           Management plane — function registry, secrets
  gateway/       Public edge — routing, auth, rate limiting
  runtime/       Serverless executor — Deno V8 isolates
  data-engine/   DB query engine — mutation recording, hooks, cron
  queue/         Async job worker — DB polling, retries, DLQ
  server/        Monolith — all five services in one binary
  cli/           `flux` CLI binary

TypeScript workspaces (package.json):
  frontend/      Marketing site + docs (Next.js)
  dashboard/     Management UI (Next.js)
  packages/functions   @flux/functions SDK
  packages/sdk         @flux/sdk
```

See [`docs/framework.md`](docs/framework.md) for the full design spec.

---

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | stable (≥ 1.80) | All Rust services |
| Node.js | ≥ 20 | Frontend / dashboard |
| PostgreSQL | ≥ 15 | Local database |
| Docker | any | Optional — `docker compose up` for full stack |

Install Rust via [rustup](https://rustup.rs). Install Node via [nvm](https://github.com/nvm-sh/nvm) or [fnm](https://github.com/Schniz/fnm).

---

## Running locally

### 1. Clone and set up the database

```bash
git clone https://github.com/flux-run/flux.git
cd flux

# Start PostgreSQL (Docker is easiest)
docker run -d --name flux-pg \
  -e POSTGRES_DB=fluxbase \
  -e POSTGRES_PASSWORD=password \
  -p 5432:5432 postgres:16

# Copy and edit the env file
cp api/.env.example api/.env   # set DATABASE_URL if needed
```

### 2. Build and run the server monolith

```bash
SQLX_OFFLINE=true cargo build -p server

LOCAL_MODE=true \
  INTERNAL_SERVICE_TOKEN=dev-token \
  DATABASE_URL="postgresql://postgres:password@localhost:5432/fluxbase" \
  ./target/debug/server
```

The server starts all five services on port **4000**.

### 3. Run the frontend (optional)

```bash
cd frontend
npm install
npm run dev    # → http://localhost:3000
```

### 4. Run the dashboard (optional)

```bash
cd dashboard
npm install
npm run dev    # → http://localhost:5173
```

### Using `make`

```bash
make dev        # API + dashboard in parallel
make api        # Run API service only
make dashboard  # Run dashboard only
make build      # Build all services
```

---

## Running tests

```bash
# All Rust tests
cargo test --workspace

# Single service
cd api && cargo test

# Single test with output
cd api && cargo test route::functions -- --nocapture

# Integration tests
make test-async-wiring    # Gateway → Queue → Worker → Runtime
make test-platform        # Full platform suite
```

---

## Submitting a pull request

1. **Fork** the repo and create a branch: `git checkout -b fix/my-bug`
2. **Make your changes** — keep them focused and small
3. **Test** — run `cargo test` and ensure CI would pass
4. **Commit** with a clear message: `fix: handle nil pointer in gateway routing`
5. **Push** and open a PR against `main`
6. Fill in the PR description — explain *what* and *why*, not just *what*

PRs that include tests for the changed behaviour are merged faster.

---

## Coding conventions

**Rust:**
- `edition = "2024"` for core services; `"2021"` for `data-engine`, `queue`, `shared/`
- `SQLX_OFFLINE=true` — use `sqlx::query_as(...)` runtime functions, not macros
- Each service follows the library + binary pattern: `src/lib.rs` + `src/main.rs`
- Errors: propagate with `?`, return `Result` from route handlers

**TypeScript:**
- Next.js App Router — server components by default, `'use client'` only when needed
- No `any` types; keep `tsconfig.json` strict
- Inline styles with CSS variables (`var(--mg-accent)` etc.) — no CSS modules

**General:**
- Don't add features or abstractions beyond what the PR requires
- Keep the single source of truth principle: all state lives in PostgreSQL
- Secrets are never logged — enforce this in any code path touching `flux.secrets`
