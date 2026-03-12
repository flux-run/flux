# CLI Reference

`flux` is the terminal interface for Flux. Every interaction with the
framework goes through it.

> For the authoritative CLI reference with status indicators, see
> [framework.md §23](framework.md#23-cli-reference).

---

## Design principles

- `flux <resource> <action>` — noun-first, verb-second
- Flags over positional args
- Every command is scriptable: `--output json`, `--confirm`, `--dry-run`
- Destructive commands require confirmation unless `--confirm` is passed
- Context stored in `~/.flux/config.json` (global)

---

## Global flags

| Flag | Default | Description |
|------|---------|-------------|
| `--output <format>` | `text` | `text`, `json`, `table` |
| `--confirm` | — | Skip confirmation prompts |
| `--dry-run` | — | Preview without executing |
| `--verbose` | — | Detailed output |
| `--quiet` | — | Suppress non-essential output |
| `--project <name>` | from `flux.toml` | Override project context |

---

## Project

```bash
flux init [name]                  # Create project with flux.toml + functions/
flux dev                          # Start all services + local Postgres + hot reload
```

`flux dev` starts Gateway (:4000), Runtime (:8083), API (:8080), Data Engine
(:8082), Queue (:8084), and an embedded Postgres. No Docker required. Watches
`functions/` for changes and hot-reloads in <200ms.

---

## Functions

```bash
flux function create <name>       # Scaffold functions/<name>/index.ts
flux function list                # List all functions in project
flux function delete <name>       # Delete a function
flux invoke <name> --data <json>  # Call a function via gateway
flux build [name]                 # Compile artifacts to .flux/build/
flux deploy [name]                # Deploy to target from flux.toml
```

Every function directory under `functions/` becomes a `POST` endpoint:
`functions/hello/` → `POST http://localhost:4000/hello`.

---

## Database

```bash
flux db push                      # Apply schemas/*.sql to DB
flux db diff                      # Preview what SQL will run (safe, never executes)
flux db migrate                   # Save diff as timestamped migration file
flux db seed                      # Apply tests/fixtures/*.sql
flux db reset                     # Drop + recreate + push + seed
```

Schemas are raw SQL files in `schemas/`. `flux db diff` compares desired state
against `information_schema` — safe to run anytime.

---

## Secrets

```bash
flux secrets set <key> <value>    # Set a secret
flux secrets get <key>            # Read a secret
flux secrets list                 # List all keys (values redacted)
flux secrets delete <key>         # Delete a secret
```

Locally stored in `.env.local` (gitignored). Never committed to version control.

---

## Observability & Debugging

```bash
flux trace <request-id>           # Full distributed trace (spans, timing, mutations)
flux trace <id> --flame           # Waterfall / flame chart visualization
flux trace list                   # List recent traces with filtering/sorting
flux trace debug <id>             # Interactive step-through mode
flux why <request-id>             # Root cause + fix suggestion (10-second diagnosis)
flux tail                         # Live request stream (htop for your backend)
flux logs <fn> --follow           # Tail function logs
flux errors                       # Per-function error summary (count, type, p95)
```

### State inspection

```bash
flux state history <table> --id <row-id>   # Full version history for a row
flux state blame <table>                   # Last writer per row, linked to request
```

### Incident replay & regression

```bash
flux incident replay <request-id>   # Re-run request with mocked side effects
flux trace diff <id-a> <id-b>       # Compare two executions field-by-field
flux bug bisect --function <name> --good <sha> --bad <sha>  # Find regression commit
```

---

## Queue & Cron

```bash
flux worker                       # Start local queue worker
flux queue list                   # List jobs (pending, running, failed)
flux queue retry <job-id>         # Retry a failed job
flux queue dead-letter            # List dead-letter jobs
flux cron list                    # List cron jobs and next fire times
```

---

## Tools & Integrations

```bash
flux add <tool>                   # Install a tool integration (Stripe, OpenAI, etc.)
flux tools list                   # List available tools
flux tools connected              # List connected tools
```

---

## Code Generation

```bash
flux generate                     # Generate TypeScript types from DB schema
```

Reads `information_schema` from the live database and emits typed accessors
for `ctx.db`. Run this after `flux db push` to update types.

---

## Status legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Implemented in CLI source |
| 🔧 | Infrastructure exists, CLI wrapper in progress |
| 📋 | Planned, not yet built |

See [framework.md §23](framework.md#23-cli-reference) for the full status table.
