# Storage

Flux uses storage as part of the product model, not just as an implementation detail.

The runtime needs durable data for execution records, deployment metadata, queue state, and debugging surfaces.

## Primary Persistent Store

Postgres is the primary persistent store for Flux.

It holds or anchors:

- project and runtime metadata
- execution records
- traces and logs metadata
- mutation history
- queue and schedule state
- deployment metadata
- operator-facing configuration

This is why Postgres sits so close to the center of the product story.

## Bundle Storage

Function bundles need durable storage so Flux can answer:

- what code version ran?
- can this execution be replayed?
- what changed between deploys?

Bundle storage may live in Postgres, object storage, or a hybrid design depending on deployment mode, but the product requirement is stable bundle identity.

## Secret Storage

Secrets are managed as part of runtime configuration, with a clear separation between:

- committed project config
- local development secrets
- production operator-managed secrets

The important rule is that secret access remains attributable within the execution model.

## Cache Layers

Flux can use caches for:

- hot function bundles
- secret lookups
- route or deployment metadata

Caches are useful for performance, but they do not break explainability. Operators can still understand which version and values were active for an execution.

## Retention

Storage policy is product policy in Flux.

Retention decisions affect:

- how far back debugging can go
- whether replay is possible
- how much mutation history is available
- how useful `why`, diff, and bisect remain

Retention is documented and operator-visible, not buried in infrastructure defaults.

## Backup And Recovery

As a source-available runtime, Flux documents backup expectations for:

- Postgres data
- bundle artifacts
- operator secrets

The product promise depends on being able to preserve and inspect execution history reliably.
