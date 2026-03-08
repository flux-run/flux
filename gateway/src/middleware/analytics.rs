use sqlx::PgPool;
use uuid::Uuid;

pub fn log_request(
    db_pool: PgPool,
    route_id: Uuid,
    tenant_id: Uuid,
    status: u16,
    latency_ms: i64,
) {
    tokio::spawn(async move {
        let metric_id = Uuid::new_v4();
        let result = sqlx::query(
            "INSERT INTO gateway_metrics (id, route_id, tenant_id, status, latency_ms) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(metric_id)
        .bind(route_id)
        .bind(tenant_id)
        .bind(i32::from(status))
        .bind(latency_ms as i32)
        .execute(&db_pool)
        .await;

        if let Err(e) = result {
            tracing::error!("Failed to append async gateway analytics: {}", e);
        }
    });
}
