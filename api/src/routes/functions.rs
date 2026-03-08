use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use crate::types::response::{ApiResponse, ApiError};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::context::RequestContext;

// ── Row structs ────────────────────────────────────────────────────────────

// Using sqlx::FromRow derive with runtime query_as to avoid SQLX_OFFLINE cache
// requirement for the new schema columns (input_schema, output_schema).
#[derive(sqlx::FromRow)]
struct FunctionRow {
    id: Uuid,
    name: String,
    runtime: String,
    description: Option<String>,
    input_schema: Option<serde_json::Value>,
    output_schema: Option<serde_json::Value>,
    created_at: chrono::NaiveDateTime,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateFunctionPayload {
    pub name: String,
    pub runtime: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err() -> ApiError {
    ApiError::internal("database_error")
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn list_functions(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let project_id = context
        .project_id
        .ok_or(ApiError::bad_request("missing_project"))?;

    let records = sqlx::query_as::<_, FunctionRow>(
        "SELECT id, name, runtime, description, input_schema, output_schema, created_at \
         FROM functions WHERE project_id = $1 ORDER BY created_at DESC"
    )
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let functions: Vec<_> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "name": r.name,
                "runtime": r.runtime,
                "description": r.description,
                "input_schema": r.input_schema,
                "output_schema": r.output_schema,
                "created_at": r.created_at.to_string()
            })
        })
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "functions": functions })))
}

pub async fn create_function(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateFunctionPayload>,
) -> ApiResult<serde_json::Value> {
    let project_id = context
        .project_id
        .ok_or(ApiError::bad_request("missing_project"))?;

    let tenant_id = context
        .tenant_id
        .ok_or(ApiError::bad_request("missing_tenant"))?;

    // Validate runtime against platform registry
    #[derive(sqlx::FromRow)]
    struct RuntimeValidationRow { _id: Uuid }
    let _runtime_valid = sqlx::query_as::<_, RuntimeValidationRow>(
        "SELECT id as _id FROM platform_runtimes WHERE name = $1 AND status = 'active'"
    )
    .bind(&payload.runtime)
    .fetch_optional(&pool)
    .await
    .map_err(|_| db_err())?
    .ok_or(ApiError::bad_request("invalid_or_inactive_runtime"))?;

    let function_id = Uuid::new_v4();

    sqlx::query!(
        "INSERT INTO functions (id, tenant_id, project_id, name, runtime) VALUES ($1, $2, $3, $4, $5)",
        function_id,
        tenant_id,
        project_id,
        payload.name,
        payload.runtime
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;
    
    // TODO: Publish to actual event bus
    println!(r#"{{"event": "function.created", "function_id": "{}", "tenant_id": "{}", "project_id": "{}"}}"#, function_id, tenant_id, project_id);

    Ok(ApiResponse::new(serde_json::json!({ "function_id": function_id })))
}

pub async fn get_function(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let project_id = context
        .project_id
        .ok_or(ApiError::bad_request("missing_project"))?;

    let record = sqlx::query_as::<_, FunctionRow>(
        "SELECT id, name, runtime, description, input_schema, output_schema, created_at \
         FROM functions WHERE id = $1 AND project_id = $2"
    )
    .bind(id)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| db_err())?
    .ok_or(ApiError::not_found("function_not_found"))?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": record.id,
        "name": record.name,
        "runtime": record.runtime,
        "description": record.description,
        "input_schema": record.input_schema,
        "output_schema": record.output_schema,
        "created_at": record.created_at.to_string()
    })))
}

pub async fn delete_function(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let project_id = context
        .project_id
        .ok_or(ApiError::bad_request("missing_project"))?;

    sqlx::query!(
        "DELETE FROM functions WHERE id = $1 AND project_id = $2",
        id,
        project_id
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}
