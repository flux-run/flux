use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use crate::types::response::{ApiResponse, ApiError};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::context::RequestContext;

// ── Row structs ────────────────────────────────────────────────────────────

struct DeploymentRow {
    id: Uuid,
    version: i32,
    is_active: bool,
    status: String,
    created_at: chrono::NaiveDateTime,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateDeploymentPayload {
    pub storage_key: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err() -> ApiError {
    ApiError::internal("database_error")
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn list_deployments(
    Path(function_id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let records = sqlx::query_as_unchecked!(
        DeploymentRow,
        "SELECT id, version, is_active, status, created_at FROM deployments WHERE function_id = $1 ORDER BY version DESC",
        function_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let deployments: Vec<_> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "version": r.version,
                "is_active": r.is_active,
                "status": r.status,
                "created_at": r.created_at.to_string()
            })
        })
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "deployments": deployments })))
}

pub async fn create_deployment(
    Path(function_id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
    Json(payload): Json<CreateDeploymentPayload>,
) -> ApiResult<serde_json::Value> {
    let deployment_id = Uuid::new_v4();

    // Get next version number
    struct VersionRow { max: Option<i32> }
    let row = sqlx::query_as_unchecked!(
        VersionRow,
        "SELECT MAX(version) as max FROM deployments WHERE function_id = $1",
        function_id
    )
    .fetch_one(&pool)
    .await
    .map_err(|_| db_err())?;

    let next_version = row.max.unwrap_or(0) + 1;

    sqlx::query!(
        "INSERT INTO deployments (id, function_id, storage_key, version, status) VALUES ($1, $2, $3, $4, 'ready')",
        deployment_id,
        function_id,
        payload.storage_key,
        next_version
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    // TODO: Publish to actual event bus
    println!(r#"{{"event": "function.deployed", "function_id": "{}", "deployment_id": "{}"}}"#, function_id, deployment_id);

    Ok(ApiResponse::new(serde_json::json!({
        "deployment_id": deployment_id,
        "version": next_version
    })))
}

pub async fn activate_deployment(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let mut tx = pool.begin().await.map_err(|_| db_err())?;

    // Find the function_id for this deployment to deactivate others
    struct DeploymentFunctionRow { function_id: Uuid }
    let fn_record = sqlx::query_as_unchecked!(
        DeploymentFunctionRow,
        "SELECT function_id FROM deployments WHERE id = $1",
        id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| db_err())?
    .ok_or(ApiError::not_found("deployment_not_found"))?;

    // Deactivate all deployments for this function
    sqlx::query!(
        "UPDATE deployments SET is_active = false WHERE function_id = $1",
        fn_record.function_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| db_err())?;

    // Activate the requested deployment
    sqlx::query!(
        "UPDATE deployments SET is_active = true WHERE id = $1",
        id
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| db_err())?;

    tx.commit().await.map_err(|_| db_err())?;

    // TODO: Publish to event bus
    println!(r#"{{"event": "function.activated", "function_id": "{}", "deployment_id": "{}"}}"#, fn_record.function_id, id);

    Ok(ApiResponse::new(serde_json::json!({ "activated": true })))
}

pub async fn deploy_function_cli(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    mut multipart: axum::extract::Multipart,
) -> ApiResult<serde_json::Value> {
    let project_id = context
        .project_id
        .ok_or(ApiError::bad_request("missing_project"))?;
    
    let tenant_id = context
        .tenant_id
        .ok_or(ApiError::bad_request("missing_tenant"))?;

    let mut name = String::new();
    let mut runtime = String::new();
    let mut bundle_bytes = Vec::new();
    let mut description: Option<String> = None;
    let mut input_schema: Option<String> = None;
    let mut output_schema: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        
        match field_name.as_str() {
            "name"    => { name         = field.text().await.unwrap_or_default(); }
            "runtime" => { runtime       = field.text().await.unwrap_or_default(); }
            "bundle"  => { bundle_bytes  = field.bytes().await.unwrap_or_default().to_vec(); }
            "description"   => { description   = field.text().await.ok().filter(|s| !s.is_empty()); }
            "input_schema"  => { input_schema   = field.text().await.ok().filter(|s| !s.is_empty()); }
            "output_schema" => { output_schema  = field.text().await.ok().filter(|s| !s.is_empty()); }
            _ => {}
        }
    }

    if name.is_empty() || runtime.is_empty() {
        return Err(ApiError::bad_request("Missing 'name' or 'runtime' fields."));
    }

    if bundle_bytes.is_empty() {
        return Err(ApiError::bad_request("Missing 'bundle' payload."));
    }

    // Attempt to locate an existing function by name and project
    #[derive(sqlx::FromRow)]
    struct FunctionLookup { id: Uuid }
    
    let existing_fn = sqlx::query_as::<_, FunctionLookup>(
        "SELECT id FROM functions WHERE name = $1 AND project_id = $2 LIMIT 1"
    )
    .bind(&name)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| db_err())?;

    let function_id = match existing_fn {
        Some(f) => {
            // Update schema metadata on re-deploy
            let input_json: Option<serde_json::Value> = input_schema.as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            let output_json: Option<serde_json::Value> = output_schema.as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            sqlx::query(
                "UPDATE functions SET description = COALESCE($1, description), \
                 input_schema = COALESCE($2::jsonb, input_schema), \
                 output_schema = COALESCE($3::jsonb, output_schema) WHERE id = $4"
            )
            .bind(description.as_deref())
            .bind(input_json)
            .bind(output_json)
            .bind(f.id)
            .execute(&pool)
            .await
            .map_err(|_| db_err())?;
            f.id
        }
        None => {
            let new_id = Uuid::new_v4();
            let input_json: Option<serde_json::Value> = input_schema.as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            let output_json: Option<serde_json::Value> = output_schema.as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            sqlx::query(
                "INSERT INTO functions (id, tenant_id, project_id, name, runtime, description, input_schema, output_schema) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
            )
            .bind(new_id)
            .bind(tenant_id)
            .bind(project_id)
            .bind(&name)
            .bind(&runtime)
            .bind(description.as_deref())
            .bind(input_json)
            .bind(output_json)
            .execute(&pool)
            .await
            .map_err(|_| db_err())?;
            new_id
        }
    };

    let deployment_id = Uuid::new_v4();
    let storage_key = format!("deployments/{}_{}.js", function_id, deployment_id);

    // Convert bundle bytes to UTF-8 string for inline storage
    let bundle_code = String::from_utf8(bundle_bytes)
        .map_err(|_| ApiError::bad_request("bundle_not_valid_utf8"))?;

    // Evaluate next deployment version
    #[derive(sqlx::FromRow)]
    struct VersionRow { max: Option<i32> }
    
    let row = sqlx::query_as::<_, VersionRow>(
        "SELECT MAX(version) as max FROM deployments WHERE function_id = $1"
    )
    .bind(function_id)
    .fetch_one(&pool)
    .await
    .map_err(|_| db_err())?;

    let next_version = row.max.unwrap_or(0) + 1;

    let mut tx = pool.begin().await.map_err(|_| db_err())?;
    
    sqlx::query(
        "UPDATE deployments SET is_active = false WHERE function_id = $1"
    )
    .bind(function_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| db_err())?;

    // Insert new deployment with actual bundle code stored inline
    sqlx::query(
        "INSERT INTO deployments (id, function_id, storage_key, bundle_code, version, status, is_active) VALUES ($1, $2, $3, $4, $5, 'ready', true)"
    )
    .bind(deployment_id)
    .bind(function_id)
    .bind(storage_key)
    .bind(&bundle_code)
    .bind(next_version)
    .execute(&mut *tx)
    .await
    .map_err(|_| db_err())?;

    tx.commit().await.map_err(|_| db_err())?;

    // Return structured payload mirroring production router architectures.
    let run_url = format!("https://run.fluxbase.co/{}", name);

    Ok(ApiResponse::new(serde_json::json!({
        "function_id": function_id,
        "deployment_id": deployment_id,
        "version": next_version,
        "url": run_url
    })))
}

// ── Internal: bundle code fetch (used by the runtime engine) ────────────────

#[derive(serde::Deserialize)]
pub struct BundleQuery {
    pub function_id: String,
}

pub async fn get_internal_bundle(
    State(pool): State<PgPool>,
    Query(params): Query<BundleQuery>,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    #[derive(sqlx::FromRow)]
    struct BundleRow { bundle_code: Option<String> }

    // Try to parse as UUID first; if that fails, treat as function name
    let row = if let Ok(fid) = params.function_id.parse::<Uuid>() {
        sqlx::query_as::<_, BundleRow>(
            "SELECT d.bundle_code FROM deployments d \
             WHERE d.function_id = $1 AND d.is_active = true \
             ORDER BY d.version DESC LIMIT 1"
        )
        .bind(fid)
        .fetch_optional(&pool)
        .await
        .map_err(|_| db_err())?
    } else {
        // Look up by function name via JOIN
        sqlx::query_as::<_, BundleRow>(
            "SELECT d.bundle_code FROM deployments d \
             JOIN functions f ON f.id = d.function_id \
             WHERE f.name = $1 AND d.is_active = true \
             ORDER BY d.version DESC LIMIT 1"
        )
        .bind(&params.function_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| db_err())?
    };

    match row {
        Some(r) => {
            match r.bundle_code {
                Some(code) => Ok(ApiResponse::new(serde_json::json!({ "code": code }))),
                None => Err(ApiError::not_found("no_bundle_found")),
            }
        }
        None => Err(ApiError::not_found("no_bundle_found")),
    }
}
