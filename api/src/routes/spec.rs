/// GET /spec
///
/// Returns a comprehensive, machine-readable project spec:
/// functions, gateway routes, connected tools, and secret names.
///
/// Designed to be the single context file loaded by AI agents,
/// CLI tooling, and dashboard documentation pages so they never
/// have to guess what exists in the project.
///
/// Example:
///   curl https://api.fluxbase.co/spec \
///     -H "Authorization: Bearer $TOKEN" \
///     -H "X-Fluxbase-Tenant: $TENANT" \
///     -H "X-Fluxbase-Project: $PROJECT"
use axum::extract::{Extension, State};
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    types::{
        context::RequestContext,
        response::{ApiError, ApiResponse},
    },
    AppState,
};

pub async fn project_spec(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
) -> Result<ApiResponse<Value>, ApiError> {
    let project_id = ctx
        .project_id
        .ok_or_else(|| ApiError::bad_request("missing_project"))?;
    let tenant_id = ctx
        .tenant_id
        .ok_or_else(|| ApiError::bad_request("missing_tenant"))?;

    // ── Project info ──────────────────────────────────────────────────────
    let project_row = sqlx::query(
        "SELECT p.id, p.name, p.slug, t.slug as tenant_slug, t.id as tenant_id \
         FROM projects p \
         JOIN tenants t ON t.id = p.tenant_id \
         WHERE p.id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| ApiError::internal("db_error"))?;

    let (project_name, project_slug, tenant_slug) = project_row
        .as_ref()
        .map(|r| (
            r.get::<String, _>("name"),
            r.get::<String, _>("slug"),
            r.get::<String, _>("tenant_slug"),
        ))
        .unwrap_or_default();

    let gateway_url = format!("https://{}.fluxbase.co", tenant_slug);
    let api_url = "https://api.fluxbase.co".to_string();

    // ── Functions ─────────────────────────────────────────────────────────
    let func_rows = sqlx::query(
        "SELECT id, name, description, input_schema, output_schema, runtime \
         FROM functions WHERE project_id = $1 ORDER BY name",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| ApiError::internal("db_error"))?;

    let functions: Vec<Value> = func_rows
        .iter()
        .map(|f| {
            let fname = f.get::<String, _>("name");
            json!({
                "id":           f.get::<uuid::Uuid, _>("id").to_string(),
                "name":         fname.clone(),
                "description":  f.get::<Option<String>, _>("description"),
                "runtime":      f.get::<String, _>("runtime"),
                "input_schema": f.get::<Option<Value>, _>("input_schema"),
                "output_schema":f.get::<Option<Value>, _>("output_schema"),
                "invoke_url":   format!("{}/{}", gateway_url, fname),
                "example": {
                    "curl": format!(
                        "curl -X POST {}/{} \\\n  -H \"Authorization: Bearer $TOKEN\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{}'",
                        gateway_url,
                        fname,
                        f.get::<Option<Value>, _>("input_schema")
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "{}".to_string()),
                    )
                }
            })
        })
        .collect();

    // ── Gateway routes ────────────────────────────────────────────────────
    let route_rows = sqlx::query(
        "SELECT r.id, r.path, r.method, r.auth_type, r.is_async, r.cors_enabled, \
                r.rate_limit, f.name as function_name \
         FROM routes r \
         JOIN functions f ON f.id = r.function_id \
         WHERE r.project_id = $1 ORDER BY r.path",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| ApiError::internal("db_error"))?;

    let routes: Vec<Value> = route_rows
        .iter()
        .map(|r| {
            let path = r.get::<String, _>("path");
            let method = r.get::<String, _>("method");
            json!({
                "id":           r.get::<uuid::Uuid, _>("id").to_string(),
                "path":         path.clone(),
                "method":       method.clone(),
                "function":     r.get::<String, _>("function_name"),
                "auth_type":    r.get::<String, _>("auth_type"),
                "is_async":     r.get::<bool, _>("is_async"),
                "cors_enabled": r.get::<bool, _>("cors_enabled"),
                "rate_limit":   r.get::<Option<i32>, _>("rate_limit"),
                "invoke_url":   format!("{}{}", gateway_url, path),
            })
        })
        .collect();

    // ── Connected tools ───────────────────────────────────────────────────
    let integration_rows = sqlx::query(
        "SELECT provider, account_label, status \
         FROM integrations WHERE project_id = $1 AND status = 'active' ORDER BY provider",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let connected_tools: Vec<Value> = integration_rows
        .iter()
        .map(|r| json!({
            "provider":      r.get::<String, _>("provider"),
            "account_label": r.get::<Option<String>, _>("account_label"),
            "status":        r.get::<String, _>("status"),
        }))
        .collect();

    // ── Secret names (not values) ─────────────────────────────────────────
    let secret_rows = sqlx::query(
        "SELECT key FROM secrets \
         WHERE (project_id = $1 OR (tenant_id = $2 AND project_id IS NULL)) \
         ORDER BY key",
    )
    .bind(project_id)
    .bind(tenant_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let secrets: Vec<String> = secret_rows
        .iter()
        .map(|r| r.get::<String, _>("key"))
        .collect();

    // ── Agent instructions ────────────────────────────────────────────────
    let instructions = json!({
        "overview": "Fluxbase is a backend runtime. Functions are deployed TypeScript handlers. \
                     Invoke them via POST to their invoke_url. DB operations go through the gateway \
                     at /db/{table}. All requests require Authorization: Bearer <token>.",
        "auth": {
            "header": "Authorization: Bearer <your-api-key>",
            "obtain": "Create an API key at https://fluxbase.co/dashboard or use: flux api-key create",
        },
        "invoke_function": {
            "method": "POST",
            "url": format!("{}/{{function_name}}", gateway_url),
            "headers": {
                "Authorization": "Bearer <token>",
                "Content-Type": "application/json",
            },
            "body": "JSON matching the function's input_schema",
        },
        "db_query": {
            "list":   format!("GET {}/db/{{table}}?limit=50&offset=0", gateway_url),
            "get":    format!("GET {}/db/{{table}}/{{id}}", gateway_url),
            "insert": format!("POST {}/db/{{table}}", gateway_url),
            "update": format!("PATCH {}/db/{{table}}/{{id}}", gateway_url),
            "delete": format!("DELETE {}/db/{{table}}/{{id}}", gateway_url),
        },
        "openapi_spec":  format!("{}/openapi.json", api_url),
        "swagger_ui":    format!("{}/openapi/ui?tenant={}&project={}", api_url, tenant_id, project_id),
        "flux_cli": {
            "deploy":  "flux deploy (run inside a function directory containing flux.json)",
            "invoke":  "flux invoke <function-name> --data '{\"key\":\"value\"}'",
            "logs":    "flux logs --function <name>",
            "trace":   "flux trace <request-id>",
            "why":     "flux why <request-id>",
            "secrets": "flux secrets list | flux secrets set KEY VALUE",
        },
    });

    Ok(ApiResponse::new(json!({
        "spec_version": "1",
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "project": {
            "id":    project_id.to_string(),
            "name":  project_name,
            "slug":  project_slug,
        },
        "tenant": {
            "id":   tenant_id.to_string(),
            "slug": tenant_slug.clone(),
        },
        "gateway_url":  gateway_url,
        "api_url":      api_url,
        "functions":    functions,
        "routes":       routes,
        "tools": {
            "connected": connected_tools,
        },
        "secrets": secrets,
        "instructions": instructions,
    })))
}
