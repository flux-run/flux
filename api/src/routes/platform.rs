use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use sqlx::PgPool;
use crate::types::context::RequestContext;

// ── Row structs ────────────────────────────────────────────────────────────

struct RuntimeRow {
    name: String,
    engine: String,
    status: String,
}

struct ServiceRow {
    name: String,
    status: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn db_err() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database_error"})))
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn list_runtimes(
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let records = sqlx::query_as_unchecked!(
        RuntimeRow,
        "SELECT name, engine, status FROM platform_runtimes WHERE status = 'active'"
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let runtimes: Vec<_> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "engine": r.engine,
                "status": r.status
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "runtimes": runtimes })))
}

pub async fn list_services(
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let records = sqlx::query_as_unchecked!(
        ServiceRow,
        "SELECT name, status FROM platform_services"
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let services: Vec<_> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "status": r.status
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "services": services })))
}
