# {name}

A [Flux](https://fluxbase.dev) project — the self-hosted backend framework where every execution is a record.

## Quick start

```bash
flux dev          # start local server + embedded Postgres on :4000
flux db push      # apply schemas/ to Postgres
flux invoke hello # test the hello function
```

## Project layout

```
{name}/
├── flux.toml          project config (port, limits, observability, cron)
├── gateway.toml       routes → functions/db, CORS, rate limits, auth
├── .env               secrets + DATABASE_URL (never committed)
├── functions/         one directory = one POST endpoint
│   └── hello/
├── schemas/           DB tables, rules, hooks — applied by flux db push
│   ├── _types.ts      shared enums
│   ├── _shared/       reusable rules + JSONB schemas
│   └── users.schema.ts
├── middleware/        request middleware
├── agents/            AI agent definitions
└── queues/            background job queues
```

## Commands

| Command | Description |
|---|---|
| `flux dev` | Start local dev server with hot reload |
| `flux db push` | Diff + apply schemas/, compile rules/hooks to DB |
| `flux db push --dry-run` | Preview SQL + AST without applying |
| `flux function create <name>` | Scaffold a new function |
| `flux deploy` | Deploy to configured target |
| `flux invoke <fn> --data '{}'` | Invoke a function locally |
| `flux trace <request-id>` | Full distributed trace of a request |
| `flux logs` | Stream live function logs |
| `flux generate` | Regenerate .flux/types from DB + functions |

## Learn more

- [Flux docs](https://docs.fluxbase.dev)
- [Schema reference](https://docs.fluxbase.dev/schemas)
- [Gateway reference](https://docs.fluxbase.dev/gateway)
