use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info};
use uuid::Uuid;
use crate::services::route_lookup::RouteRecord;

#[derive(Default, Clone)]
pub struct SnapshotData {
    pub tenants_by_slug: HashMap<String, Uuid>,
    // RouteKey is (tenant_id, method, path)
    pub routes: HashMap<(Uuid, String, String), RouteRecord>,
}

#[derive(Clone)]
pub struct GatewaySnapshot {
    pub data: Arc<RwLock<Arc<SnapshotData>>>,
    pub db_pool: PgPool,
}

impl GatewaySnapshot {
    pub fn new(db_pool: PgPool) -> Self {
        Self {
            data: Arc::new(RwLock::new(Arc::new(SnapshotData::default()))),
            db_pool,
        }
    }

    pub async fn get_data(&self) -> Arc<SnapshotData> {
        self.data.read().await.clone()
    }

    pub async fn refresh(&self) -> anyhow::Result<()> {
        let mut new_data = SnapshotData::default();

        // Fetch tenants
        #[derive(sqlx::FromRow)]
        struct TenantRow { slug: String, id: Uuid }

        let tenants = sqlx::query_as::<_, TenantRow>("SELECT slug, id FROM tenants")
            .fetch_all(&self.db_pool)
            .await?;

        for t in tenants {
            new_data.tenants_by_slug.insert(t.slug, t.id);
        }

        // Fetch routes
        let routes = sqlx::query_as::<_, RouteRecord>(
            "SELECT r.id, r.project_id, p.tenant_id, r.path, r.method, r.function_id, r.is_async, r.auth_type, r.cors_enabled, r.rate_limit, \
             r.jwks_url, r.jwt_audience, r.jwt_issuer, r.json_schema, r.cors_origins, r.cors_headers \
             FROM routes r \
             JOIN projects p ON p.id = r.project_id"
        )
        .fetch_all(&self.db_pool)
        .await?;

        for r in routes {
            let key = (r.tenant_id, r.method.clone(), r.path.clone());
            new_data.routes.insert(key, r);
        }

        // Atomic swap
        {
            let mut write_guard = self.data.write().await;
            *write_guard = Arc::new(new_data);
        }

        Ok(())
    }

    pub fn start_background_refresh(snapshot: Self) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = snapshot.refresh().await {
                    error!("Failed to refresh gateway routing snapshot: {:?}", e);
                } else {
                    info!("Successfully refreshed routing snapshot");
                }
            }
        });
    }
}
