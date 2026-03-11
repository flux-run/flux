/// Execution-plane OpenAPI / docs / agent-schema endpoints.
///
/// These routes live on the tenant subdomain: {tenant}.fluxbase.co
///
/// GET /openapi.json   — Full OpenAPI 3.0 spec: functions + DB CRUD + routes
/// GET /docs           — Swagger UI, pre-authenticated via ?key=&project= params
/// GET /agent-schema   — Compact LLM-optimised schema (no OpenAPI verbosity)
///
/// Tenant is resolved from the Host header subdomain (same as identity_resolver).
/// No auth guard on the docs/spec themselves — the content describes what the
/// tenant has exposed and what auth each endpoint requires.
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::state::SharedState;

// ─── Tenant-slug extraction ───────────────────────────────────────────────────
// Duplicates the logic in identity_resolver so these routes don't need the
// identity middleware (they are purely informational, not execution paths).

fn extract_tenant_slug(headers: &HeaderMap) -> Option<String> {
    let raw = if let Some(t) = headers.get("x-tenant").and_then(|h| h.to_str().ok()) {
        t.to_string()
    } else {
        let host = headers
            .get("x-forwarded-host")
            .or_else(|| headers.get("host"))
            .and_then(|h| h.to_str().ok())?;
        host.split('.').next()?.to_string()
    };
    let slug: String = raw.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let collapsed = slug.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    if collapsed.is_empty() { None } else { Some(collapsed) }
}

// ─── DB query helpers ─────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct FunctionRow {
    id:           Uuid,
    name:         String,
    description:  Option<String>,
    input_schema: Option<Value>,
    output_schema: Option<Value>,
}

#[derive(sqlx::FromRow)]
struct RouteRow {
    path:         String,
    method:       String,
    auth_type:    String,
    function_name: String,
    json_schema:  Option<Value>,
}

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id:   Uuid,
    slug: String,
    name: String,
}

// ─── Schema column metadata from data engine ─────────────────────────────────

#[derive(sqlx::FromRow)]
struct ColMeta {
    table_name:  String,
    column_name: String,
    fb_type:     String,
    #[allow(dead_code)]
    pg_type:     String,
}

// ─── Shared data loader ───────────────────────────────────────────────────────

struct TenantExecData {
    tenant_id:    Uuid,
    tenant_slug:  String,
    gateway_url:  String,
    projects:     Vec<ProjectRow>,
    functions:    Vec<FunctionRow>,
    routes:       Vec<RouteRow>,
    tables:       Vec<(String, Vec<ColMeta>)>,   // (table_name, columns)
}

async fn load_tenant_data(state: &SharedState, headers: &HeaderMap) -> Result<TenantExecData, Response> {
    let slug = extract_tenant_slug(headers).ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "unable to determine tenant"}))).into_response()
    })?;

    let snapshot = state.snapshot.get_data().await;
    let tenant_id = *snapshot.tenants_by_slug.get(&slug).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "tenant not found"}))).into_response()
    })?;

    let gateway_url = format!("https://{}.fluxbase.co", slug);

    // Projects for this tenant
    let projects: Vec<ProjectRow> = sqlx::query_as::<_, ProjectRow>(
        "SELECT id, slug, name FROM projects WHERE tenant_id = $1 ORDER BY created_at",
    )
    .bind(tenant_id)
    .fetch_all(&state.db_pool)
    .await
    .map_err(|e| {
        tracing::error!("openapi: projects query failed: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
    })?;

    let project_ids: Vec<Uuid> = projects.iter().map(|p| p.id).collect();
    if project_ids.is_empty() {
        // Tenant has no projects yet — return empty spec, not an error
        return Ok(TenantExecData {
            tenant_id,
            tenant_slug: slug,
            gateway_url,
            projects,
            functions: vec![],
            routes: vec![],
            tables: vec![],
        });
    }

    // Functions (all projects for this tenant)
    let functions: Vec<FunctionRow> = sqlx::query_as::<_, FunctionRow>(
        r#"SELECT f.id, f.name, f.description, f.input_schema, f.output_schema
           FROM functions f
           WHERE f.project_id = ANY($1)
           ORDER BY f.name"#,
    )
    .bind(&project_ids)
    .fetch_all(&state.db_pool)
    .await
    .map_err(|e| {
        tracing::error!("openapi: functions query failed: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
    })?;

    // Routes joined with function name
    let routes: Vec<RouteRow> = sqlx::query_as::<_, RouteRow>(
        r#"SELECT r.path, r.method, r.auth_type, f.name AS function_name, r.json_schema
           FROM routes r
           JOIN functions f ON f.id = r.function_id
           JOIN projects p ON p.id = r.project_id
           WHERE p.tenant_id = $1
           ORDER BY r.path, r.method"#,
    )
    .bind(tenant_id)
    .fetch_all(&state.db_pool)
    .await
    .map_err(|e| {
        tracing::error!("openapi: routes query failed: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
    })?;

    // DB columns from data engine's column_metadata table
    // Group by table so we can produce per-table schema objects.
    let raw_cols: Vec<ColMeta> = sqlx::query_as::<_, ColMeta>(
        r#"SELECT table_name, column_name, fb_type, pg_type
           FROM fluxbase_internal.column_metadata
           WHERE tenant_id = $1 AND project_id = ANY($2)
           ORDER BY table_name, ordinal"#,
    )
    .bind(tenant_id)
    .bind(&project_ids)
    .fetch_all(&state.db_pool)
    .await
    .unwrap_or_default(); // Schema may legitimately not exist yet

    // Group columns by table (sorted by table name for stable output)
    let mut table_map: std::collections::HashMap<String, Vec<ColMeta>> = std::collections::HashMap::new();
    for col in raw_cols {
        table_map.entry(col.table_name.clone()).or_default().push(col);
    }
    let mut tables: Vec<(String, Vec<ColMeta>)> = table_map.into_iter().collect();
    tables.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(TenantExecData {
        tenant_id,
        tenant_slug: slug,
        gateway_url,
        projects,
        functions,
        routes,
        tables,
    })
}

// ─── OpenAPI 3.0 generator ────────────────────────────────────────────────────

fn fb_type_to_json_schema(ft: &str) -> Value {
    match ft {
        "int" | "integer" | "bigint" | "int8" | "int4" | "int2" => json!({"type":"integer"}),
        "float" | "float4" | "float8" | "numeric" | "decimal" => json!({"type":"number"}),
        "bool" | "boolean" => json!({"type":"boolean"}),
        "timestamp" | "timestamptz" | "date" | "time" => json!({"type":"string","format":"date-time"}),
        "uuid" => json!({"type":"string","format":"uuid"}),
        "json" | "jsonb" => json!({"type":"object","additionalProperties":true}),
        "text[]" | "varchar[]" => json!({"type":"array","items":{"type":"string"}}),
        _ => json!({"type":"string"}),
    }
}

fn to_pascal(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

fn build_openapi(data: &TenantExecData) -> Value {
    let mut schemas: Map<String, Value> = Map::new();
    let mut paths:   Map<String, Value> = Map::new();

    // ── DB CRUD paths ─────────────────────────────────────────────────────
    for (table_name, cols) in &data.tables {
        let pascal = to_pascal(table_name);

        // Build column properties
        let mut props: Map<String, Value> = Map::new();
        let mut required_write: Vec<String> = Vec::new();
        let auto_cols = ["id", "created_at", "updated_at", "deleted_at"];

        for col in cols {
            let schema = fb_type_to_json_schema(&col.fb_type);
            props.insert(col.column_name.clone(), schema);
            if !auto_cols.contains(&col.column_name.as_str()) {
                required_write.push(col.column_name.clone());
            }
        }

        // Row schema (select / list responses)
        schemas.insert(format!("{}Row", pascal), json!({
            "type": "object",
            "properties": props,
        }));

        // Write schema (insert / update body)
        let write_props: Map<String, Value> = props
            .iter()
            .filter(|(k, _)| !auto_cols.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        schemas.insert(format!("{}Write", pascal), json!({
            "type": "object",
            "properties": write_props,
            "required": required_write,
        }));

        // POST /db/{table}/insert
        paths.insert(format!("/db/{}/insert", table_name), json!({
            "post": {
                "tags": ["Database"],
                "summary": format!("Insert row into {}", table_name),
                "operationId": format!("db_{}_insert", table_name),
                "security": [{"projectKey":[]}],
                "requestBody": {
                    "required": true,
                    "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{}Write", pascal)}}}
                },
                "responses": {
                    "200": {"description":"Row inserted","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}Row", pascal)}}}},
                    "400": {"description":"Validation error"},
                    "401": {"description":"Unauthorized"}
                }
            }
        }));

        // GET /db/{table}/select
        paths.insert(format!("/db/{}/select", table_name), json!({
            "get": {
                "tags": ["Database"],
                "summary": format!("Query rows from {}", table_name),
                "operationId": format!("db_{}_select", table_name),
                "security": [{"projectKey":[]}],
                "parameters": [
                    {"name":"filter","in":"query","schema":{"type":"string"},"description":"SQL WHERE condition subset (safe predicates only)"},
                    {"name":"limit","in":"query","schema":{"type":"integer","default":100,"maximum":1000}},
                    {"name":"offset","in":"query","schema":{"type":"integer","default":0}},
                    {"name":"order_by","in":"query","schema":{"type":"string"}}
                ],
                "responses": {
                    "200": {"description":"Row list","content":{"application/json":{"schema":{"type":"array","items":{"$ref":format!("#/components/schemas/{}Row", pascal)}}}}}
                }
            }
        }));

        // POST /db/{table}/update
        paths.insert(format!("/db/{}/update", table_name), json!({
            "post": {
                "tags": ["Database"],
                "summary": format!("Update rows in {}", table_name),
                "operationId": format!("db_{}_update", table_name),
                "security": [{"projectKey":[]}],
                "requestBody": {
                    "required": true,
                    "content": {"application/json": {"schema": {
                        "type": "object",
                        "required": ["filter", "values"],
                        "properties": {
                            "filter": {"type":"object","description":"Conditions to match"},
                            "values": {"$ref": format!("#/components/schemas/{}Write", pascal)}
                        }
                    }}}
                },
                "responses": {
                    "200": {"description":"Rows updated","content":{"application/json":{"schema":{"type":"object","properties":{"updated":{"type":"integer"}}}}}}
                }
            }
        }));

        // POST /db/{table}/delete
        paths.insert(format!("/db/{}/delete", table_name), json!({
            "post": {
                "tags": ["Database"],
                "summary": format!("Delete rows from {}", table_name),
                "operationId": format!("db_{}_delete", table_name),
                "security": [{"projectKey":[]}],
                "requestBody": {
                    "required": true,
                    "content": {"application/json": {"schema": {
                        "type": "object",
                        "required": ["filter"],
                        "properties": {
                            "filter": {"type":"object","description":"Conditions to match (required for safety)"}
                        }
                    }}}
                },
                "responses": {
                    "200": {"description":"Rows deleted","content":{"application/json":{"schema":{"type":"object","properties":{"deleted":{"type":"integer"}}}}}}
                }
            }
        }));
    }

    // ── Raw SQL query ─────────────────────────────────────────────────────
    paths.insert("/db/query".to_string(), json!({
        "post": {
            "tags": ["Database"],
            "summary": "Execute a raw SQL query",
            "operationId": "db_query",
            "security": [{"projectKey":[]}],
            "requestBody": {
                "required": true,
                "content": {"application/json": {"schema": {
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": {"type":"string","description":"SQL query string"},
                        "params": {"type":"array","items":{},"description":"Positional bind parameters"}
                    }
                }}}
            },
            "responses": {
                "200": {"description":"Query results","content":{"application/json":{"schema":{"type":"object","properties":{"rows":{"type":"array","items":{"type":"object"}},"row_count":{"type":"integer"}}}}}}
            }
        }
    }));

    // ── Function invocation paths ─────────────────────────────────────────
    // Use route table (exposed paths) — fall back to /run/{name} for functions
    // that have a registered route.
    let mut routed_fns: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for route in &data.routes {
        let path = if route.path.starts_with('/') {
            route.path.clone()
        } else {
            format!("/{}", route.path)
        };

        let func = match data.functions.iter().find(|f| f.name == route.function_name) {
            Some(f) => f,
            None    => continue,
        };
        let input_schema  = func.input_schema.clone().unwrap_or(json!({"type":"object"}));
        let output_schema = func.output_schema.clone().unwrap_or(json!({"type":"object"}));
        let description   = func.description.clone().unwrap_or_else(|| format!("Invoke function {}", route.function_name));

        routed_fns.insert(func.name.as_str());

        let security: Value = match route.auth_type.as_str() {
            "none" | "public" => json!([]),
            "jwt"  => json!([{"bearerJwt":[]}]),
            _      => json!([{"projectKey":[]}]),
        };

        let pascal = to_pascal(&route.function_name);
        schemas.insert(format!("{}Input", pascal),  input_schema.clone());
        schemas.insert(format!("{}Output", pascal), output_schema.clone());

        let method = route.method.to_lowercase();
        let operation = json!({
            "tags": ["Functions"],
            "summary": description,
            "operationId": format!("fn_{}_{}", method, route.function_name.replace('-', "_")),
            "security": security,
            "requestBody": if method == "post" || method == "put" || method == "patch" {
                json!({
                    "required": true,
                    "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{}Input", pascal)}}}
                })
            } else { Value::Null },
            "responses": {
                "200": {
                    "description": "Success",
                    "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{}Output", pascal)}}}
                },
                "400": {"description":"Validation error"},
                "401": {"description":"Unauthorized"},
                "422": {"description":"Function error"}
            }
        });

        let path_entry = paths.entry(path).or_insert(json!({}));
        if let Value::Object(map) = path_entry {
            map.insert(method, operation);
        }
    }

    // Functions without explicit routes (callable via internal or not yet routed)
    for func in &data.functions {
        if routed_fns.contains(func.name.as_str()) { continue; }
        let pascal = to_pascal(&func.name);
        let input  = func.input_schema.clone().unwrap_or(json!({"type":"object"}));
        let output = func.output_schema.clone().unwrap_or(json!({"type":"object"}));
        schemas.insert(format!("{}Input",  pascal), input.clone());
        schemas.insert(format!("{}Output", pascal), output.clone());
        // Document as POST /run/{name}
        paths.insert(format!("/run/{}", func.name), json!({
            "post": {
                "tags": ["Functions"],
                "summary": func.description.clone().unwrap_or_else(|| format!("Invoke {}", func.name)),
                "operationId": format!("run_{}", func.name.replace('-', "_")),
                "security": [{"projectKey":[]}],
                "requestBody": {
                    "required": true,
                    "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{}Input", pascal)}}}
                },
                "responses": {
                    "200": {"description":"Success","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}Output", pascal)}}}},
                    "401": {"description":"Unauthorized"},
                    "422": {"description":"Function error"}
                }
            }
        }));
    }

    json!({
        "openapi": "3.0.3",
        "info": {
            "title": format!("{} — Execution API", data.tenant_slug),
            "description": format!(
                "Auto-generated execution-plane API for tenant **{}**.\n\n\
                 Includes function invocations, database CRUD, and workflow endpoints.\n\
                 Authenticate with a project API key (`Authorization: Bearer <key>`).",
                data.tenant_slug
            ),
            "version": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        },
        "servers": [
            {"url": data.gateway_url, "description": format!("{} gateway", data.tenant_slug)}
        ],
        "components": {
            "securitySchemes": {
                "projectKey": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "Fluxbase project API key",
                    "description": "Project API key — create one at api.fluxbase.co/api-keys"
                },
                "bearerJwt": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "JWT",
                    "description": "JWT token — validated against the route's configured JWKS endpoint"
                }
            },
            "schemas": schemas,
            "responses": {
                "Unauthorized": {
                    "description": "Missing or invalid Authorization header",
                    "content": {"application/json": {"schema": {"type":"object","properties":{"error":{"type":"string"}}}}}
                },
                "NotFound": {
                    "description": "Resource not found",
                    "content": {"application/json": {"schema": {"type":"object","properties":{"error":{"type":"string"}}}}}
                }
            }
        },
        "paths": paths,
    })
}

// ─── GET /openapi.json ────────────────────────────────────────────────────────

pub async fn openapi_json(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Response {
    match load_tenant_data(&state, &headers).await {
        Err(r) => r,
        Ok(data) => {
            let spec = build_openapi(&data);
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                axum::Json(spec),
            )
                .into_response()
        }
    }
}

// ─── Swagger UI ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DocsQuery {
    /// Project API key — pre-populated into the Authorize dialog
    pub key:     Option<String>,
    /// Project UUID hint (informational display only)
    pub project: Option<String>,
}

/// GET /docs
///
/// Serves a dark-mode Swagger UI scoped to this tenant's execution plane.
/// Authenticate with `?key=flux_...` to pre-populate the bearer token.
pub async fn docs_ui(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(params): Query<DocsQuery>,
) -> Response {
    let slug = match extract_tenant_slug(&headers) {
        Some(s) => s,
        None => return (StatusCode::BAD_REQUEST, "Unable to determine tenant").into_response(),
    };

    // Verify tenant exists
    {
        let snapshot = state.snapshot.get_data().await;
        if !snapshot.tenants_by_slug.contains_key(&slug) {
            return (StatusCode::NOT_FOUND, Html("<h1>Tenant not found</h1>")).into_response();
        }
    }

    let api_key  = params.key.unwrap_or_default();
    let project  = params.project.unwrap_or_default();
    let (has_key, auth_color, auth_label) = if api_key.is_empty() {
        (false, "#f87171", "⚠ No API key — use ?key=YOUR_PROJECT_KEY")
    } else {
        (true, "#4ade80", "✓ API key loaded")
    };

    let project_badge = if !project.is_empty() {
        format!(r#"<span class="badge">project {}</span>"#, &project[..8.min(project.len())])
    } else {
        String::new()
    };

    let preauth_js = if has_key {
        format!(r#"ui.preauthorizeApiKey('projectKey', '{}');"#, api_key.replace('\'', "\\'"))
    } else {
        String::new()
    };

    let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1.0">
  <title>{slug} — API Docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
  <style>
    *{{box-sizing:border-box;margin:0;padding:0}}
    body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#0d0d0d;color:#e0e0e0}}
    #top-bar{{background:#111;border-bottom:1px solid #222;padding:12px 24px;display:flex;align-items:center;gap:16px}}
    #top-bar .logo{{font-weight:700;font-size:1rem;color:#fff;letter-spacing:-.02em}}
    #top-bar .tenant{{font-size:.82rem;color:#a78bfa;font-family:monospace}}
    #top-bar .badge{{font-size:.72rem;background:#1a1a1a;border:1px solid #333;border-radius:4px;padding:2px 8px;color:#aaa;font-family:monospace}}
    #top-bar .auth-status{{margin-left:auto;font-size:.78rem;color:{auth_color}}}
    #swagger-ui{{max-width:1200px;margin:0 auto;padding:24px}}
    .swagger-ui .topbar{{display:none}}
    .swagger-ui{{color:#e0e0e0}}
    .swagger-ui .info .title{{color:#fff}}
    .swagger-ui .scheme-container{{background:#111;border-bottom:1px solid #222}}
    .swagger-ui .opblock-tag{{color:#ccc;border-bottom:1px solid #222}}
    .swagger-ui input[type=text],.swagger-ui textarea{{background:#1a1a1a;color:#e0e0e0;border-color:#333}}
    .swagger-ui select{{background:#1a1a1a;color:#e0e0e0;border-color:#333}}
    .swagger-ui .btn{{border-color:#444;color:#ccc}}
    .swagger-ui .btn.authorize{{border-color:#4ade80;color:#4ade80}}
    .swagger-ui .auth-wrapper{{background:#111;border-color:#333}}
    .swagger-ui section.models{{background:#111;border-color:#222}}
    .swagger-ui .model-box{{background:#0d0d0d}}
    .swagger-ui .opblock{{background:#111;border-color:#222}}
    .swagger-ui .opblock.opblock-get{{border-color:#1d4ed8;background:rgba(29,78,216,.06)}}
    .swagger-ui .opblock.opblock-post{{border-color:#15803d;background:rgba(21,128,61,.06)}}
    .swagger-ui .opblock.opblock-patch{{border-color:#b45309;background:rgba(180,83,9,.06)}}
    .swagger-ui .opblock.opblock-delete{{border-color:#b91c1c;background:rgba(185,28,28,.06)}}
  </style>
</head>
<body>
  <div id="top-bar">
    <span class="logo">Fluxbase</span>
    <span class="tenant">{slug}</span>
    <span class="badge">Execution API</span>
    {project_badge}
    <span class="auth-status">{auth_label}</span>
  </div>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    const API_KEY = {api_key_js};
    const ui = SwaggerUIBundle({{
      url: '/openapi.json',
      dom_id: '#swagger-ui',
      presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
      layout: 'BaseLayout',
      deepLinking: true,
      defaultModelsExpandDepth: 1,
      displayRequestDuration: true,
      filter: true,
      requestInterceptor: (req) => {{
        if (API_KEY) req.headers['Authorization'] = 'Bearer ' + API_KEY;
        return req;
      }},
      onComplete: () => {{
        {preauth_js}
        // Patch "Try it out" base URL to the real gateway
        if (ui.specSelectors && ui.specSelectors.servers) {{
          // swagger-ui uses the spec servers[] array — already set correctly in openapi.json
        }}
      }}
    }});
    window.ui = ui;
  </script>
</body>
</html>"#,
        slug       = slug,
        auth_color = auth_color,
        auth_label = auth_label,
        project_badge = project_badge,
        api_key_js = if api_key.is_empty() { "null".to_string() } else { format!("'{}'", api_key.replace('\'', "\\'")) },
        preauth_js = preauth_js,
    );

    Html(html).into_response()
}

// ─── GET /agent-schema ────────────────────────────────────────────────────────

/// Compact, LLM-optimised schema — returns only what an AI agent needs to know,
/// without OpenAPI boilerplate.  Fits in ~800 tokens for a typical tenant.
///
/// GET /agent-schema
pub async fn agent_schema(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Response {
    let data = match load_tenant_data(&state, &headers).await {
        Err(r) => return r,
        Ok(d)  => d,
    };

    // Functions — name + description + input fields + output fields
    let functions: Vec<Value> = data.functions.iter().map(|f| {
        let input_fields = extract_field_names(f.input_schema.as_ref());
        let output_fields = extract_field_names(f.output_schema.as_ref());
        // Find its route path if any
        let route_path = data.routes.iter()
            .find(|r| r.function_name == f.name)
            .map(|r| r.path.clone());
        json!({
            "name": f.name,
            "description": f.description,
            "invoke": route_path.unwrap_or_else(|| format!("/run/{}", f.name)),
            "input": input_fields,
            "output": output_fields,
        })
    }).collect();

    // Tables — name + column names + types
    let tables: Vec<Value> = data.tables.iter().map(|(table_name, cols)| {
        let columns: Vec<Value> = cols.iter().map(|c| json!({
            "name": c.column_name,
            "type": c.fb_type,
        })).collect();
        json!({
            "name": table_name,
            "crud": {
                "insert": format!("/db/{}/insert", table_name),
                "select": format!("/db/{}/select", table_name),
                "update": format!("/db/{}/update", table_name),
                "delete": format!("/db/{}/delete", table_name),
            },
            "columns": columns,
        })
    }).collect();

    // Routes summary
    let routes: Vec<Value> = data.routes.iter().map(|r| json!({
        "method": r.method,
        "path": r.path,
        "function": r.function_name,
        "auth": r.auth_type,
    })).collect();

    // Agent instructions block
    let instructions = json!({
        "auth": "Add header: Authorization: Bearer <project_api_key>",
        "base_url": data.gateway_url,
        "invoke_function": format!("POST {}/{{route_path}}  body: {{...input}}", data.gateway_url),
        "query_db": format!("POST {}/db/query  body: {{\"query\":\"SELECT...\"}}", data.gateway_url),
        "full_spec": format!("{}/openapi.json", data.gateway_url),
        "swagger_ui": format!("{}/docs?key=YOUR_PROJECT_KEY", data.gateway_url),
    });

    let schema = json!({
        "schema_version": "1",
        "tenant": data.tenant_slug,
        "gateway": data.gateway_url,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "functions": functions,
        "tables": tables,
        "routes": routes,
        "instructions": instructions,
    });

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        axum::Json(schema),
    )
        .into_response()
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn extract_field_names(schema: Option<&Value>) -> Vec<String> {
    schema
        .and_then(|s| s.get("properties"))
        .and_then(|p| p.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default()
}
