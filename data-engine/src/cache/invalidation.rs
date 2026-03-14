//! Cross-instance cache invalidation via Postgres LISTEN/NOTIFY.
//!
//! When multiple data-engine instances run behind a load balancer, each holds
//! its own in-process Moka caches (`schema_cache`, `plan_cache`). A DDL
//! mutation handled by instance A would leave B's caches stale for up to the
//! TTL. A Postgres trigger on `table_metadata`, `column_metadata`, and
//! `relationships` fires `NOTIFY flux_cache_changes` with a JSON payload
//! describing the narrowest invalidation required.
//!
//! Payload shapes:
//!
//! | Payload                                   | Handler                      |
//! |-------------------------------------------|------------------------------|
//! | `{"type":"table","schema":"…","table":"…"}`| `invalidate_table(s, t)`     |
//! | `{"type":"schema","schema":"…"}`           | `invalidate_schema(s)`       |
//! | `{"type":"all"}`                           | `invalidate_all()`           |
//!
//! On reconnect the caches are fully cleared to ensure consistency.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use sqlx::postgres::PgListener;
use tracing::{error, info, warn};

use crate::state::AppState;

const NOTIFY_CHANNEL: &str = "flux_cache_changes";

#[derive(Deserialize)]
struct InvalidationPayload {
    #[serde(rename = "type")]
    kind:   String,
    schema: Option<String>,
    table:  Option<String>,
}

pub fn start_listener(app_state: Arc<AppState>, database_url: String) {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);

        loop {
            match PgListener::connect(&database_url).await {
                Err(e) => {
                    error!("Cache NOTIFY listener failed to connect: {:?} (retry in {:?})", e, backoff);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    continue;
                }

                Ok(mut listener) => {
                    backoff = Duration::from_secs(1);

                    if let Err(e) = listener.listen(NOTIFY_CHANNEL).await {
                        error!("Cache NOTIFY LISTEN failed: {:?}", e);
                        continue;
                    }

                    app_state.cache.invalidate_all();
                    info!(channel = NOTIFY_CHANNEL, "Cache invalidation listener ready");

                    loop {
                        match listener.recv().await {
                            Ok(notification) => apply(&app_state, notification.payload()),
                            Err(e) => {
                                warn!("Cache NOTIFY connection lost: {:?} — reconnecting", e);
                                break;
                            }
                        }
                    }
                }
            }
        }
    });
}

fn apply(app_state: &AppState, payload: &str) {
    let p: InvalidationPayload = match serde_json::from_str(payload) {
        Ok(p) => p,
        Err(e) => {
            warn!(payload, "Failed to parse cache invalidation payload: {:?}", e);
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

        "all" => {
            info!("cache invalidation: all");
            app_state.cache.invalidate_all();
        }

        other => {
            warn!(kind = other, "Unknown cache invalidation type");
        }
    }
}
