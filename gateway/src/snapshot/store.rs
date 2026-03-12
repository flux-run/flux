//! In-memory route snapshot.
//!
//! Loaded once at startup, then kept current via Postgres LISTEN/NOTIFY.
//! No polling — any INSERT/UPDATE/DELETE on `routes` fires a notification
//! (see migration `20260312000029_route_notify_trigger.sql`) and the
//! snapshot is refreshed immediately.
//!
//! On reconnect after a dropped NOTIFY connection the snapshot is refreshed
//! immediately to catch any changes that arrived during the gap.
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use super::types::{RouteRecord, SnapshotData};

/// Postgres channel name — must match the trigger function in the migration.
const NOTIFY_CHANNEL: &str = "route_changes";

/// Wraps the snapshot in an `Arc<RwLock>` so all Axum workers share one copy
/// with cheap clones and wait-free reads during the common case.
#[derive(Clone)]
pub struct GatewaySnapshot {
    data:         Arc<RwLock<Arc<SnapshotData>>>,
    db_pool:      PgPool,
    database_url: String,
}

impl GatewaySnapshot {
    pub fn new(db_pool: PgPool, database_url: String) -> Self {
        Self {
            data: Arc::new(RwLock::new(Arc::new(SnapshotData::default()))),
            db_pool,
            database_url,
        }
    }

    /// Read the current snapshot.  Lock-free in the common case.
    pub async fn get_data(&self) -> Arc<SnapshotData> {
        self.data.read().await.clone()
    }

    /// Pull routes from the database and atomically swap the snapshot.
    pub async fn refresh(&self) -> anyhow::Result<()> {
        let rows = sqlx::query_as::<_, RouteRecord>(
            "SELECT r.id, r.project_id, r.function_id, r.path, r.method,
                    COALESCE(f.runtime, 'deno') AS runtime,
                    r.auth_type, r.cors_enabled, r.rate_limit,
                    r.jwks_url, r.jwt_audience, r.jwt_issuer,
                    r.json_schema, r.cors_origins, r.cors_headers
             FROM   routes r
             JOIN   functions f ON f.id = r.function_id",
        )
        .fetch_all(&self.db_pool)
        .await?;

        let mut data = SnapshotData::default();
        for r in rows {
            data.routes.insert((r.method.to_uppercase(), r.path.clone()), r);
        }
        *self.data.write().await = Arc::new(data);
        Ok(())
    }

    /// Spawn a background task that listens for `NOTIFY route_changes` from
    /// Postgres and refreshes the snapshot immediately when a notification
    /// arrives.
    ///
    /// The listener uses its own dedicated connection (required by
    /// `PgListener`) separate from the pool.  On connection drop it
    /// reconnects automatically with exponential back-off.
    pub fn start_notify_listener(snapshot: Self) {
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);

            loop {
                match PgListener::connect(&snapshot.database_url).await {
                    Err(e) => {
                        error!("NOTIFY listener failed to connect: {:?} (retry in {:?})", e, backoff);
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                        continue;
                    }
                    Ok(mut listener) => {
                        backoff = Duration::from_secs(1); // reset on successful connect

                        if let Err(e) = listener.listen(NOTIFY_CHANNEL).await {
                            error!("NOTIFY LISTEN failed: {:?}", e);
                            continue;
                        }

                        // Refresh immediately — changes may have arrived
                        // while the connection was down.
                        if let Err(e) = snapshot.refresh().await {
                            error!("Snapshot refresh on (re)connect failed: {:?}", e);
                        }

                        info!("Listening for route changes on Postgres channel '{}'", NOTIFY_CHANNEL);

                        loop {
                            match listener.recv().await {
                                Ok(notification) => {
                                    info!(
                                        "Route change detected ({}), refreshing snapshot",
                                        notification.payload()
                                    );
                                    if let Err(e) = snapshot.refresh().await {
                                        error!("Snapshot refresh after NOTIFY failed: {:?}", e);
                                    }
                                }
                                Err(e) => {
                                    warn!("NOTIFY listener connection lost: {:?} — reconnecting", e);
                                    break; // reconnect outer loop
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
