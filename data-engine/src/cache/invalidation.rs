//! Cross-instance cache invalidation via Postgres LISTEN/NOTIFY.
//!
//! ## Problem
//!
//! Flux is documented as stateless and horizontally scalable.  When multiple
//! data-engine instances run behind a load balancer, each holds its own
//! in-process [Moka] caches (`schema_cache`, `plan_cache`, `policy_cache`).
//! A DDL mutation handled by instance A would leave B's caches stale for up
//! to the TTL (60 s schema, 300 s plan).
//!
//! ## Solution
//!
//! A Postgres trigger on `policies`, `table_metadata`, `column_metadata`,
//! `relationships`, and `hooks` fires `NOTIFY flux_cache_changes` with a
//! JSON payload describing the narrowest invalidation required.  This
//! function starts a dedicated [`PgListener`] background task on every
//! instance that receives the notification and calls the matching
//! [`CacheManager`] method immediately.
//!
//! Payload shapes (from `20260312000018_cache_invalidation_trigger.sql`):
//!
//! | Payload                                   | Handler                      |
//! |-------------------------------------------|------------------------------|
//! | `{"type":"table","schema":"…","table":"…"}`| `invalidate_table(s, t)`     |
//! | `{"type":"schema","schema":"…"}`           | `invalidate_schema(s)`       |
//! | `{"type":"policy"}`                        | `invalidate_policy()`        |
//! | `{"type":"all"}`                           | `invalidate_all()`           |
//!
//! On reconnect after a dropped connection the caches are fully cleared to
//! ensure consistency (changes may have arrived during the gap).
//!
//! The local instance's mutation handlers still call `invalidate_*` directly
//! for zero-latency local invalidation — the NOTIFY ensures *remote*
//! instances are also updated.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use sqlx::postgres::PgListener;
use tracing::{error, info, warn};

use crate::state::AppState;

/// Postgres channel name — must match the trigger in the migration.
const NOTIFY_CHANNEL: &str = "flux_cache_changes";

/// Typed representation of the NOTIFY payload JSON.
#[derive(Deserialize)]
struct InvalidationPayload {
    #[serde(rename = "type")]
    kind:   String,
    schema: Option<String>,
    table:  Option<String>,
}

/// Spawn a background task that listens on `flux_cache_changes` and
/// invalidates the in-process caches whenever a notification arrives.
///
/// Uses exponential back-off (1 s → 30 s) on connection failure and performs
/// a full [`CacheManager::invalidate_all`] on every (re)connect to flush any
/// changes that arrived while the connection was down.
pub fn start_listener(app_state: Arc<AppState>, database_url: String) {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);

        loop {
            match PgListener::connect(&database_url).await {
                Err(e) => {
                    error!(
                        "Cache NOTIFY listener failed to connect: {:?} (retry in {:?})",
                        e, backoff
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    continue;
                }

                Ok(mut listener) => {
                    backoff = Duration::from_secs(1); // reset on success

                    if let Err(e) = listener.listen(NOTIFY_CHANNEL).await {
                        error!("Cache NOTIFY LISTEN failed: {:?}", e);
                        continue;
                    }

                    // Flush everything on (re)connect — we may have missed
                    // notifications while the connection was down.
                    app_state.cache.invalidate_all();
                    app_state.cache.invalidate_policy().await;

                    info!(
                        channel = NOTIFY_CHANNEL,
                        "Cache invalidation listener ready"
                    );

                    loop {
                        match listener.recv().await {
                            Ok(notification) => {
                                apply(&app_state, notification.payload()).await;
                            }
                            Err(e) => {
                                warn!(
                                    "Cache NOTIFY connection lost: {:?} — reconnecting",
                                    e
                                );
                                break; // outer loop → reconnect
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Deserialize the payload and call the narrowest invalidation possible.
async fn apply(app_state: &AppState, payload: &str) {
    let p: InvalidationPayload = match serde_json::from_str(payload) {
        Ok(p) => p,
        Err(e) => {
            warn!(
                payload,
                "Failed to parse cache invalidation payload: {:?}", e
            );
            return;
        }
    };

    match p.kind.as_str() {
        "table" => {
            if let (Some(schema), Some(table)) = (p.schema.as_deref(), p.table.as_deref()) {
                info!(schema, table, "cache invalidation: table");
                app_state.cache.invalidate_table(schema, table);
            } else {
                warn!(payload, "cache invalidation 'table' payload missing schema/table");
            }
        }

        "schema" => {
            if let Some(schema) = p.schema.as_deref() {
                info!(schema, "cache invalidation: schema");
                app_state.cache.invalidate_schema(schema);
            } else {
                warn!(payload, "cache invalidation 'schema' payload missing schema");
            }
        }

        "policy" => {
            info!("cache invalidation: policy");
            app_state.cache.invalidate_policy().await;
        }

        "all" => {
            info!("cache invalidation: all");
            app_state.cache.invalidate_all();
        }

        other => {
            warn!(kind = other, "Unknown cache invalidation type");
        }
    }
}
