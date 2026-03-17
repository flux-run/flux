# Bundled Artifacts

Flux v1 should be treated as a bundled-artifact platform.

The supported developer path is:

```bash
flux build app.ts
flux run app.ts --listen
```

`flux build` walks the module graph, fetches supported remote dependencies, and writes a deterministic artifact to `.flux/artifact.json` by default. `flux run --listen` then executes that built graph inside `flux-runtime`.

## Why This Is The Official Path

- Avoids runtime-side npm and CDN resolution complexity.
- Lets Flux snapshot the exact module graph that was executed.
- Keeps replay tied to a stable artifact hash instead of an open-ended loader environment.
- Matches the product goal: ship an app quickly, then debug it deterministically.

## What The Builder Supports Today

- local ESM modules via relative imports
- `https://...` imports during build
- `npm:` imports during build, fetched through `esm.sh`

The runtime itself is not a general Deno-style remote loader. Remote and `npm:` modules are expected to be captured into the built artifact first.

## What To Avoid For v1

- relying on raw runtime-side npm resolution
- relying on runtime-side remote URL fetching
- designing around `require()` or CommonJS

## Product Model

Think of Flux as:

```text
User code -> flux build -> Flux artifact -> flux-runtime -> replay/debugging
```

Not as:

```text
User code -> ad hoc runtime loader -> arbitrary package environment
```

## Launch Goal

The launch-ready claim is not “full module loader compatibility.”

It is:

"Bundle a Hono + Drizzle app, run it on Flux, and replay executions deterministically."