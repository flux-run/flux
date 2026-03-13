# hello-c

A minimal Flux function written in **Ruby (WASM via ruby.wasm)**.

## What it does

Returns `{ "ok": true }` — the simplest valid Flux function.
Edit `functions/hello/rb` to add real logic.

## Run it

```bash
# Start the local dev server (zero config — Postgres included)
flux dev

# In another terminal, deploy and invoke
flux deploy
flux invoke hello
```

## Prerequisites

- [Flux CLI](https://fluxbase.dev/docs/install)


## Project structure

```
hello-c/
├── flux.toml              # project config (commit this)
└── functions/
    └── hello/
        ├── flux.json      # input/output schema + validation
        └── *              # function source file(s)
```
