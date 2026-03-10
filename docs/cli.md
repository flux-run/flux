# CLI Reference

All commands follow the pattern `flux <command> [options]`.

Global flags available on every command:

| Flag | Description |
|---|---|
| `--project <id>` | Override the active project |
| `--tenant <slug>` | Override the active tenant |
| `--api-url <url>` | Override the API base URL |
| `--json` | Output raw JSON (useful for scripting) |

---

## auth

```bash
flux auth login          # Open browser-based OAuth flow
flux auth logout         # Remove local credentials
flux auth status         # Show the currently authenticated user
```

---

## init

```bash
flux init [directory]    # Bootstrap a new function project
```

Creates `flux.json` in the current (or given) directory and links it to a new
project on the Fluxbase control plane.

---

## deploy

```bash
flux deploy [directory]
```

Bundles the function at `directory` (default: `.`), uploads it to the control
plane, and sets it as the live revision.  The CLI prints the new deployment ID.

Options:

| Flag | Description |
|---|---|
| `--dry-run` | Bundle locally but do not upload |
| `--name <name>` | Override the function name |

---

## invoke

```bash
flux invoke <function-name> [options]
```

Call a deployed function and print the response.

Options:

| Flag | Description |
|---|---|
| `--data <json>` | JSON payload string |
| `--file <path>` | Read payload from a JSON file |
| `--async` | Enqueue as an async job (returns job ID) |

Example:

```bash
flux invoke greet --data '{"name": "Alice"}'
flux invoke process_order --file payload.json --async
```

---

## functions

```bash
flux functions list            # List all deployed functions
flux functions get <name>      # Show function metadata
flux functions delete <name>   # Delete a function
```

---

## deployments

```bash
flux deployments list [function-name]     # List deployments
flux deployments get <deployment-id>      # Show a specific deployment
```

---

## logs

```bash
flux logs <function-name> [options]
```

Stream or page through platform logs for a function.

Options:

| Flag | Description |
|---|---|
| `--limit <n>` | Max number of log lines to show (default: 50) |
| `--level <level>` | Filter by level: `debug`, `info`, `warn`, `error` |
| `--since <duration>` | e.g. `30m`, `1h`, `24h` |
| `--follow` / `-f` | Tail live output |

---

## trace

```bash
flux trace <request-id> [options]
```

Display the full distributed trace for a request.  Output includes all spans,
slow span annotations, N+1 warnings, slow DB warnings, and index suggestions.

Options:

| Flag | Description |
|---|---|
| `--slow-threshold <ms>` | Slow span cutoff in ms (default: 500) |
| `--flame` | Render a proportional flame graph waterfall |
| `--json` | Print the raw trace JSON from the API |

Examples:

```bash
flux trace a3f9d2b1-4c8e-4f7d-b2e1-9d0c3a5f8e2b
flux trace a3f9d2b1-... --flame
flux trace a3f9d2b1-... --slow-threshold 100 --json
```

---

## secrets

```bash
flux secrets set <key> <value>      # Create or update a secret
flux secrets list                   # List all keys (values hidden)
flux secrets delete <key>           # Remove a secret
```

Secrets are encrypted at rest and injected into the function context as
`ctx.secrets.get(key)` / `ctx.env[key]`.

---

## projects

```bash
flux projects list          # List all projects in the active tenant
flux projects get <id>      # Show project details
flux projects switch <id>   # Set as the active project
```

---

## sdk

```bash
flux sdk generate           # Download the typed SDK for the current project
```

Writes a `fluxbase.d.ts` TypeScript declaration file that augments
`@fluxbase/sdk` with your current schema (tables and functions).

---

## doctor

```bash
flux doctor
```

Runs a self-diagnostic: checks CLI version, auth status, API reachability, and
active project configuration.  Good first step when something isn't working.

---

## dev

```bash
flux dev [directory]
```

Local development server that hot-reloads your function on file changes.
Simulates the production runtime (Deno isolate) locally without deploying.

```bash
flux dev .
# Function available at http://localhost:8080/greet
```

---

## Configuration

The CLI stores credentials and active project context in
`~/.fluxbase/config.json`:

```json
{
  "api_url":     "https://api.fluxbase.co",
  "gateway_url": "https://YOUR_GATEWAY_URL",
  "tenant_slug": "acme",
  "project_id":  "3787e1fa-..."
}
```

Override any field with environment variables:

```bash
FLUXBASE_API_URL=...
FLUXBASE_GATEWAY_URL=...
FLUXBASE_TENANT=...
FLUXBASE_PROJECT=...
```
