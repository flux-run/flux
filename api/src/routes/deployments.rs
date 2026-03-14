//! Deployment routes — function bundle upload, deployment management, and
//! the internal bundle-fetch endpoint used by the runtime.
//!
//! ## Deploy flow (CLI → API → Filesystem + DB → Runtime)
//! ```text
//! flux deploy
//!   └─ POST /functions/deploy  (multipart: name, runtime, bundle)
//!        ├─ Upsert function record in DB (metadata only)
//!        ├─ Write bundle bytes to FLUX_FUNCTIONS_DIR/{name}.js|.wasm
//!        ├─ Insert deployment row (version++) — no bundle_code in DB
//!        └─ Deactivate old deployments, activate new one
//! ```
//!
//! ## Bundle storage
//! Bundles live on the **filesystem** at `FLUX_FUNCTIONS_DIR/{name}.{ext}`:
//!   - Dev:        `{project_root}/.flux/build/`   (set by `flux dev`)
//!   - Production: `/app/functions/`               (baked into Docker image)
//!
//! Postgres stores only metadata (name, route, runtime, schemas, bundle_hash).
//! The `bundle_hash` column is kept for incremental-deploy detection in the CLI.

use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use crate::error::{ApiError, ApiResponse, ApiResult};
use api_contract::deployments::{CreateDeploymentPayload, CreateProjectDeploymentPayload};
use sqlx::PgPool;
use serde::Deserialize;
use uuid::Uuid;
use crate::types::context::RequestContext;
use crate::AppState;

// ── Row structs ─────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DeploymentRow {
    id:            Uuid,
    version:       i32,
    is_active:     bool,
    status:        String,
    created_at:    chrono::NaiveDateTime,
    function_name: String,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn run_url(gateway_url: &str, name: &str) -> String {
    format!("{}/{}", gateway_url.trim_end_matches('/'), name)
}

// ── Handlers ────────────────────────────────────────────────────────────────

pub async fn list_deployments(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Path(function_name): Path<String>,
) -> ApiResult<serde_json::Value> {
    let records = sqlx::query_as::<_, DeploymentRow>(
        "SELECT d.id, d.version, d.is_active, d.status, d.created_at, f.name as function_name \
         FROM deployments d \
         JOIN functions f ON f.id = d.function_id \
         WHERE f.name = $1 \
         ORDER BY d.version DESC",
    )
    .bind(&function_name)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let deployments: Vec<_> = records
        .into_iter()
        .map(|r| {
            let url = if r.is_active {
                Some(run_url(&state.gateway_url, &r.function_name))
            } else {
                None
            };
            serde_json::json!({
                "id":         r.id,
                "version":    r.version,
                "is_active":  r.is_active,
                "status":     r.status,
                "created_at": r.created_at.to_string(),
                "run_url":    url,
            })
        })
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "deployments": deployments })))
}

pub async fn create_deployment(
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
    Path(function_id): Path<Uuid>,
    Json(payload): Json<CreateDeploymentPayload>,
) -> ApiResult<serde_json::Value> {
    let deployment_id = Uuid::new_v4();

    #[derive(sqlx::FromRow)]
    struct VersionRow { max: Option<i32> }
    let row = sqlx::query_as::<_, VersionRow>(
        "SELECT MAX(version) as max FROM deployments WHERE function_id = $1",
    )
    .bind(function_id)
    .fetch_one(&pool)
    .await
    .map_err(ApiError::from)?;

    let next_version = row.max.unwrap_or(0) + 1;

    sqlx::query(
        "INSERT INTO deployments (id, function_id, storage_key, version, status) \
         VALUES ($1, $2, $3, $4, 'ready')",
    )
    .bind(deployment_id)
    .bind(function_id)
    .bind(&payload.storage_key)
    .bind(next_version)
    .execute(&pool)
    .await
    .map_err(ApiError::from)?;

    Ok(ApiResponse::created(serde_json::json!({
        "deployment_id": deployment_id,
        "version":       next_version,
    })))
}

pub async fn activate_deployment(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
    Path((function_name, version)): Path<(String, i32)>,
) -> ApiResult<serde_json::Value> {
    #[derive(sqlx::FromRow)]
    struct DeploymentFunctionRow { deployment_id: Uuid, function_id: Uuid }

    let mut tx = pool.begin().await.map_err(ApiError::from)?;

    let fn_record = sqlx::query_as::<_, DeploymentFunctionRow>(
        "SELECT d.id as deployment_id, f.id as function_id \
         FROM deployments d \
         JOIN functions f ON f.id = d.function_id \
         WHERE f.name = $1 AND d.version = $2",
    )
    .bind(&function_name)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::from)?
    .ok_or_else(|| ApiError::not_found("deployment not found"))?;

    sqlx::query("UPDATE deployments SET is_active = false WHERE function_id = $1")
        .bind(fn_record.function_id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::from)?;

    sqlx::query("UPDATE deployments SET is_active = true WHERE id = $1")
        .bind(fn_record.deployment_id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::from)?;

    tx.commit().await.map_err(ApiError::from)?;

    Ok(ApiResponse::new(serde_json::json!({
        "activated": true,
        "version":   version,
    })))
}

/// `POST /functions/deploy` — CLI deploy endpoint.
///
/// Accepts a multipart form with fields:
///   - `name`          — function name (required)
///   - `runtime`       — runtime identifier (default: "deno")
///   - `bundle`        — compiled JS bundle bytes (required)
///   - `description`   — optional human description
///   - `input_schema`  — optional JSON Schema for input validation
///   - `output_schema` — optional JSON Schema for output validation
pub async fn deploy_function_cli(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    mut multipart: axum::extract::Multipart,
) -> ApiResult<serde_json::Value> {
    let mut name         = String::new();
    let mut runtime      = String::new();
    let mut bundle_bytes = Vec::<u8>::new();
    let mut description:   Option<String> = None;
    let mut input_schema:  Option<String> = None;
    let mut output_schema: Option<String> = None;
    let mut bundle_hash:   Option<String> = None;
    let mut project_deployment_id: Option<Uuid> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name().unwrap_or("") {
            "name"                   => name         = field.text().await.unwrap_or_default(),
            "runtime"                => runtime      = field.text().await.unwrap_or_default(),
            "bundle"                 => bundle_bytes = field.bytes().await.unwrap_or_default().to_vec(),
            "description"            => description  = field.text().await.ok().filter(|s| !s.is_empty()),
            "input_schema"           => input_schema  = field.text().await.ok().filter(|s| !s.is_empty()),
            "output_schema"          => output_schema = field.text().await.ok().filter(|s| !s.is_empty()),
            "bundle_hash"            => bundle_hash   = field.text().await.ok().filter(|s| !s.is_empty()),
            "project_deployment_id"  => {
                project_deployment_id = field.text().await.ok()
                    .filter(|s| !s.is_empty())
                    .and_then(|s| s.parse::<Uuid>().ok());
            }
            _                        => {}
        }
    }

    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    if bundle_bytes.is_empty() {
        return Err(ApiError::bad_request("bundle is required"));
    }
    if runtime.is_empty() {
        runtime = "deno".to_string();
    }

    // ── Upsert function record ────────────────────────────────────────────

    #[derive(sqlx::FromRow)]
    struct FunctionLookup { id: Uuid }

    let existing = sqlx::query_as::<_, FunctionLookup>(
        "SELECT id FROM functions WHERE name = $1 LIMIT 1",
    )
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let input_json:  Option<serde_json::Value> = input_schema.as_deref().and_then(|s| serde_json::from_str(s).ok());
    let output_json: Option<serde_json::Value> = output_schema.as_deref().and_then(|s| serde_json::from_str(s).ok());

    let function_id = match existing {
        Some(f) => {
            // Update schema metadata on re-deploy
            sqlx::query(
                "UPDATE functions \
                 SET description = COALESCE($1, description), \
                     input_schema  = COALESCE($2::jsonb, input_schema), \
                     output_schema = COALESCE($3::jsonb, output_schema) \
                 WHERE id = $4",
            )
            .bind(description.as_deref())
            .bind(&input_json)
            .bind(&output_json)
            .bind(f.id)
            .execute(&state.pool)
            .await
            .map_err(ApiError::from)?;
            f.id
        }
        None => {
            let new_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO functions \
                     (id, name, runtime, description, input_schema, output_schema) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(new_id)
            .bind(&name)
            .bind(&runtime)
            .bind(description.as_deref())
            .bind(&input_json)
            .bind(&output_json)
            .execute(&state.pool)
            .await
            .map_err(ApiError::from)?;
            new_id
        }
    };

    // ── Bundle storage: write to filesystem ──────────────────────────────
    //
    // The bundle lives at {FLUX_FUNCTIONS_DIR}/{name}.js|.wasm, not in Postgres.
    // The runtime reads it from disk on every cold start (cached in memory after).

    let ext = if runtime == "wasm" { "wasm" } else { "js" };
    let functions_dir = std::path::Path::new(&state.functions_dir);
    std::fs::create_dir_all(functions_dir)
        .map_err(|e| ApiError::internal(format!("cannot create functions_dir: {e}")))?;

    let bundle_path = functions_dir.join(format!("{name}.{ext}"));
    std::fs::write(&bundle_path, &bundle_bytes)
        .map_err(|e| ApiError::internal(format!("failed to write bundle to disk: {e}")))?;

    tracing::debug!(path = %bundle_path.display(), "bundle written to filesystem");

    // ── Deployment record ─────────────────────────────────────────────────

    let deployment_id = Uuid::new_v4();
    // storage_key records the relative bundle filename for diagnostics.
    let storage_key = format!("{name}.{ext}");

    #[derive(sqlx::FromRow)]
    struct VersionRow { max: Option<i32> }
    let row = sqlx::query_as::<_, VersionRow>(
        "SELECT MAX(version) as max FROM deployments WHERE function_id = $1",
    )
    .bind(function_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let next_version = row.max.unwrap_or(0) + 1;

    let mut tx = state.pool.begin().await.map_err(ApiError::from)?;

    sqlx::query("UPDATE deployments SET is_active = false WHERE function_id = $1")
        .bind(function_id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::from)?;

    sqlx::query(
        "INSERT INTO deployments \
             (id, function_id, storage_key, version, status, is_active, bundle_hash, project_deployment_id) \
         VALUES ($1, $2, $3, $4, 'ready', true, $5, $6)",
    )
    .bind(deployment_id)
    .bind(function_id)
    .bind(&storage_key)
    .bind(next_version)
    .bind(&bundle_hash)
    .bind(project_deployment_id)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::from)?;

    tx.commit().await.map_err(ApiError::from)?;

    tracing::info!(
        function_id  = %function_id,
        deployment_id = %deployment_id,
        version      = next_version,
        name         = %name,
        "function deployed",
    );

    Ok(ApiResponse::created(serde_json::json!({
        "function_id":   function_id,
        "deployment_id": deployment_id,
        "version":       next_version,
        "bundle_hash":   bundle_hash,
        "run_url":       run_url(&state.gateway_url, &name),
    })))
}

// ── Internal: bundle fetch (used by the runtime engine) ─────────────────────

#[derive(Deserialize)]
pub struct BundleQuery {
    pub function_id: String,
}

pub async fn get_internal_bundle(
    State(state): State<AppState>,
    Query(params): Query<BundleQuery>,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    #[derive(sqlx::FromRow)]
    struct BundleRow {
        id:           Uuid,
        name:         String,
        runtime:      String,
        input_schema: Option<serde_json::Value>,
        output_schema: Option<serde_json::Value>,
    }

    let row = if let Ok(fid) = params.function_id.parse::<Uuid>() {
        sqlx::query_as::<_, BundleRow>(
            "SELECT d.id, f.name, f.runtime, \
                    f.input_schema, f.output_schema \
             FROM deployments d \
             JOIN functions f ON f.id = d.function_id \
             WHERE d.function_id = $1 AND d.is_active = true \
             ORDER BY d.version DESC LIMIT 1",
        )
        .bind(fid)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::from)?
    } else {
        sqlx::query_as::<_, BundleRow>(
            "SELECT d.id, f.name, f.runtime, \
                    f.input_schema, f.output_schema \
             FROM deployments d \
             JOIN functions f ON f.id = d.function_id \
             WHERE f.name = $1 AND d.is_active = true \
             ORDER BY d.version DESC LIMIT 1",
        )
        .bind(&params.function_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::from)?
    };

    let r = row.ok_or_else(|| ApiError::not_found("no active deployment found"))?;

    // Read bundle from filesystem — bundles live at {FLUX_FUNCTIONS_DIR}/{name}.{ext}
    let ext = if r.runtime == "wasm" { "wasm" } else { "js" };
    let bundle_path = std::path::Path::new(&state.functions_dir).join(format!("{}.{}", r.name, ext));

    // WASM bundles are binary — base64-encode for JSON transport.
    // JS bundles are UTF-8 text — read directly.
    let code = if r.runtime == "wasm" {
        let bytes = std::fs::read(&bundle_path).map_err(|e| {
            tracing::warn!(
                path = %bundle_path.display(),
                function = %r.name,
                "bundle file not found on filesystem: {e}"
            );
            ApiError::not_found("bundle file not found — deploy the function first")
        })?;
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    } else {
        std::fs::read_to_string(&bundle_path).map_err(|e| {
            tracing::warn!(
                path = %bundle_path.display(),
                function = %r.name,
                "bundle file not found on filesystem: {e}"
            );
            ApiError::not_found("bundle file not found — deploy the function first")
        })?
    };

    Ok(ApiResponse::new(serde_json::json!({
        "deployment_id": r.id,
        "runtime":       r.runtime,
        "code":          code,
        "input_schema":  r.input_schema,
        "output_schema": r.output_schema,
    })))
}

// ── New project-level deploy handlers ────────────────────────────────────────

/// `GET /deployments/hashes` — return the active bundle_hash for every
/// function in the project so the CLI can skip unchanged functions.
pub async fn get_deployment_hashes(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    #[derive(sqlx::FromRow)]
    struct HashRow {
        name:        String,
        bundle_hash: Option<String>,
    }

    let rows = sqlx::query_as::<_, HashRow>(
        "SELECT f.name, d.bundle_hash \
         FROM functions f \
         JOIN deployments d ON d.function_id = f.id AND d.is_active = true",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let hashes: serde_json::Map<String, serde_json::Value> = rows
        .into_iter()
        .filter_map(|r| r.bundle_hash.map(|h| (r.name, serde_json::Value::String(h))))
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "hashes": hashes })))
}



/// `POST /deployments/project` — record a project-level deployment after the
/// CLI finishes uploading individual functions.
pub async fn create_project_deployment(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<CreateProjectDeploymentPayload>,
) -> ApiResult<serde_json::Value> {
    let id = Uuid::new_v4();
    let deployed_by = payload.deployed_by.unwrap_or_else(|| "cli".into());
    let summary_json = serde_json::to_value(&serde_json::json!({
        "total":     payload.summary.total,
        "deployed":  payload.summary.deployed,
        "skipped":   payload.summary.skipped,
        "functions": payload.summary.functions
            .iter()
            .map(|f| serde_json::json!({ "name": f.name, "version": f.version, "status": f.status }))
            .collect::<Vec<_>>(),
    }))
    .unwrap_or_default();

    #[derive(sqlx::FromRow)]
    struct CreatedAt { created_at: chrono::DateTime<chrono::Utc> }

    let row = sqlx::query_as::<_, CreatedAt>(
        "INSERT INTO project_deployments (id, version, summary, deployed_by) \
         VALUES ($1, $2, $3, $4) \
         RETURNING created_at",
    )
    .bind(id)
    .bind(payload.version as i32)
    .bind(&summary_json)
    .bind(&deployed_by)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    Ok(ApiResponse::created(serde_json::json!({
        "id":         id,
        "version":    payload.version,
        "created_at": row.created_at.to_rfc3339(),
    })))
}

/// `GET /deployments/project` — list recent project deployments (paginated).
pub async fn list_project_deployments(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<crate::validation::PaginationQuery>,
) -> ApiResult<serde_json::Value> {
    #[derive(sqlx::FromRow)]
    struct ProjectDepRow {
        id:          Uuid,
        version:     i32,
        summary:     serde_json::Value,
        deployed_by: String,
        created_at:  chrono::DateTime<chrono::Utc>,
    }

    let (limit, offset) = page.clamped();

    let rows = sqlx::query_as::<_, ProjectDepRow>(
        "SELECT id, version, summary, deployed_by, created_at \
         FROM project_deployments \
         ORDER BY version DESC \
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let deployments: Vec<_> = rows
        .into_iter()
        .map(|r| serde_json::json!({
            "id":          r.id,
            "version":     r.version,
            "summary":     r.summary,
            "deployed_by": r.deployed_by,
            "created_at":  r.created_at.to_rfc3339(),
        }))
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "deployments": deployments })))
}

/// `POST /deployments/project/:id/rollback` — re-activate all function
/// deployments from a previous project deployment.
pub async fn rollback_project_deployment(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    axum::extract::Path(project_deployment_id): axum::extract::Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    // Load the project deployment and verify ownership.
    #[derive(sqlx::FromRow)]
    struct ProjDepRow {
        version: i32,
        summary: serde_json::Value,
    }

    let proj = sqlx::query_as::<_, ProjDepRow>(
        "SELECT version, summary \
         FROM project_deployments \
         WHERE id = $1",
    )
    .bind(project_deployment_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?
    .ok_or_else(|| ApiError::not_found("project deployment not found"))?;

    // Collect functions that were actually deployed (not skipped).
    let functions: Vec<(String,)> = proj.summary
        .get("functions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|f| f.get("status").and_then(|s| s.as_str()) == Some("deployed"))
                .filter_map(|f| f.get("name").and_then(|n| n.as_str()).map(|n| (n.to_owned(),)))
                .collect()
        })
        .unwrap_or_default();

    let mut functions_restored: i64 = 0;
    let mut tx = state.pool.begin().await.map_err(ApiError::from)?;

    for (fn_name,) in &functions {
        // Resolve the function_id.
        #[derive(sqlx::FromRow)]
        struct FnId { id: Uuid }

        let fn_row = sqlx::query_as::<_, FnId>(
            "SELECT id FROM functions WHERE name = $1",
        )
        .bind(fn_name)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ApiError::from)?;

        let Some(fn_row) = fn_row else { continue };

        // Deactivate all deployments for this function.
        sqlx::query("UPDATE deployments SET is_active = false WHERE function_id = $1")
            .bind(fn_row.id)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::from)?;

        // Activate the deployment from this project deployment.
        let updated = sqlx::query(
            "UPDATE deployments SET is_active = true \
             WHERE project_deployment_id = $1 AND function_id = $2",
        )
        .bind(project_deployment_id)
        .bind(fn_row.id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::from)?;

        if updated.rows_affected() > 0 {
            functions_restored += 1;
        }
    }

    tx.commit().await.map_err(ApiError::from)?;

    Ok(ApiResponse::new(serde_json::json!({
        "rolled_back_to":    proj.version,
        "functions_restored": functions_restored,
    })))
}
