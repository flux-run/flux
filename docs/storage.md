# Storage

All Flux state lives in PostgreSQL. No Redis, no S3, no external object stores.

## Primary Persistent Store

Postgres holds:

- project and runtime metadata
- execution records
- traces and logs metadata
- mutation history
- queue and schedule state
- deployment metadata and inline bundle code
- operator-facing configuration

## Bundle Storage

Function bundles are stored inline in the `deployments.bundle_code` column. Bundles are built at deploy time from source code and stored directly in Postgres.

This enables:

- what code version ran? (`bundle_hash` column)
- can this execution be replayed? (yes — code is in the DB alongside the execution record)
- what changed between deploys? (diff two `bundle_code` values)

No external object storage is required.

## Secret Storage

Secrets are managed as part of runtime configuration, with a clear separation between:

- committed project config
- local development secrets
- production operator-managed secrets

Secret access is attributable within the execution model.

## Cache Layers

Flux uses caches for:

- hot function bundles (LRU with 60s TTL)
- secret lookups (LRU with 30s TTL)
- route and deployment metadata

Caches improve performance but do not break explainability.

## Retention

Retention decisions affect:

- how far back debugging can go
- whether replay is possible
- how much mutation history is available
- how useful `why`, diff, and bisect remain

Retention is documented and operator-visible.

## Backup and Recovery

For backup, operators run standard PostgreSQL backup tools (`pg_dump`, WAL archiving, or managed DB snapshots). Functions live in source code — the DB holds the built bundles, but the source of truth is the code repository.
