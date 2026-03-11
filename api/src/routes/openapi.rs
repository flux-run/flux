/// OpenAPI 3.0 generator.
///
/// GET /openapi.json  — machine-readable spec (project-scoped, authenticated)
/// GET /openapi/ui    — Swagger UI browser, project-bounded + pre-authenticated
///                      via ?token=&tenant=&project= query params
///
/// The spec is generated from the live schema graph (DB tables + functions)
/// and is suitable for Postman, Insomnia, Swagger UI, and code generators.
use axum::extract::{Extension, Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    types::{
        context::RequestContext,
        response::ApiError,
    },
    AppState,
};

use super::sdk::fetch_schema_graph_pub;

// ─── Swagger UI page ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UiQuery {
    /// API key or Bearer token — pre-populated as the default Authorization value
    pub token:   Option<String>,
    /// Tenant UUID — injected into every request header automatically
    pub tenant:  Option<String>,
    /// Project UUID — injected into every request header automatically
    pub project: Option<String>,
}

/// GET /openapi/ui
///
/// Serves an interactive Swagger UI page scoped to a single project.
/// The client is fully authenticated: the token, tenant ID, and project ID
/// are injected via a requestInterceptor into every request Swagger makes,
/// including the initial spec load.
///
/// Share URL format:
///   https://api.fluxbase.co/openapi/ui?token=flux_...&tenant=<tid>&project=<pid>
pub async fn ui(Query(params): Query<UiQuery>) -> impl IntoResponse {
    let token   = params.token.unwrap_or_default();
    let tenant  = params.tenant.unwrap_or_default();
    let project = params.project.unwrap_or_default();

    let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Fluxbase API Explorer</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #0d0d0d; color: #e0e0e0; }}
    #top-bar {{
      background: #111; border-bottom: 1px solid #222;
      padding: 12px 24px; display: flex; align-items: center; gap: 16px;
    }}
    #top-bar .logo {{ font-weight: 700; font-size: 1rem; color: #fff; letter-spacing: -.02em; }}
    #top-bar .badge {{
      font-size: .72rem; background: #1a1a1a; border: 1px solid #333;
      border-radius: 4px; padding: 2px 8px; color: #aaa; font-family: monospace;
    }}
    #top-bar .auth-status {{
      margin-left: auto; font-size: .78rem;
      color: {auth_color};
    }}
    #swagger-ui {{ max-width: 1200px; margin: 0 auto; padding: 24px; }}
    /* Dark-mode overrides */
    .swagger-ui .topbar {{ display: none; }}
    .swagger-ui {{ color: #e0e0e0; }}
    .swagger-ui .info .title {{ color: #fff; }}
    .swagger-ui .scheme-container {{ background: #111; border-bottom: 1px solid #222; }}
    .swagger-ui .opblock-tag {{ color: #ccc; border-bottom: 1px solid #222; }}
    .swagger-ui .opblock .opblock-summary-method {{ font-weight: 700; }}
    .swagger-ui input[type=text], .swagger-ui textarea {{ background: #1a1a1a; color: #e0e0e0; border-color: #333; }}
    .swagger-ui select {{ background: #1a1a1a; color: #e0e0e0; border-color: #333; }}
    .swagger-ui .btn {{ border-color: #444; color: #ccc; }}
    .swagger-ui .btn.authorize {{ border-color: #4ade80; color: #4ade80; }}
    .swagger-ui .auth-wrapper {{ background: #111; border-color: #333; }}
    .swagger-ui section.models {{ background: #111; border-color: #222; }}
    .swagger-ui .model-box {{ background: #0d0d0d; }}
    .swagger-ui .opblock {{ background: #111; border-color: #222; }}
    .swagger-ui .opblock.opblock-get {{ border-color: #1d4ed8; background: rgba(29,78,216,.06); }}
    .swagger-ui .opblock.opblock-post {{ border-color: #15803d; background: rgba(21,128,61,.06); }}
    .swagger-ui .opblock.opblock-patch {{ border-color: #b45309; background: rgba(180,83,9,.06); }}
    .swagger-ui .opblock.opblock-delete {{ border-color: #b91c1c; background: rgba(185,28,28,.06); }}
  </style>
</head>
<body>
  <div id="top-bar">
    <span class="logo">Fluxbase</span>
    <span class="badge">API Explorer</span>
    {project_badge}
    <span class="auth-status">{auth_label}</span>
  </div>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-standalone-preset.js"></script>
  <script>
    const TOKEN   = {token_js};
    const TENANT  = {tenant_js};
    const PROJECT = {project_js};

    const ui = SwaggerUIBundle({{
      url:     '/openapi.json',
      dom_id:  '#swagger-ui',
      presets: [SwaggerUIBundle.presets.apis, SwaggerUIStandalonePreset],
      layout:  'StandaloneLayout',
      persistAuthorization: true,
      deepLinking: true,
      defaultModelsExpandDepth: 1,
      defaultModelExpandDepth: 2,

      // Inject auth headers into every request, including the spec load itself.
      requestInterceptor: (req) => {{
        if (TOKEN)   req.headers['Authorization']       = 'Bearer ' + TOKEN;
        if (TENANT)  req.headers['X-Fluxbase-Tenant']  = TENANT;
        if (PROJECT) req.headers['X-Fluxbase-Project'] = PROJECT;
        return req;
      }},

      onComplete: () => {{
        // Pre-populate the Authorization field in Swagger's auth dialog
        if (TOKEN) {{
          ui.preauthorizeApiKey('bearerAuth', TOKEN);
        }}
      }},
    }});
  </script>
</body>
</html>
"#,
        auth_color   = if token.is_empty() { "#f87171" } else { "#4ade80" },
        auth_label   = if token.is_empty() {
            "⚠ No token — add ?token=flux_... to the URL".to_string()
        } else {
            format!("✓ Authenticated · token …{}", &token[token.len().saturating_sub(6)..])
        },
        project_badge = if project.is_empty() { String::new() } else {
            format!("<span class=\"badge\">project: {}…</span>", &project[..8.min(project.len())])
        },
        token_js   = if token.is_empty()   { "null".to_string() } else { format!("'{}'", token) },
        tenant_js  = if tenant.is_empty()  { "null".to_string() } else { format!("'{}'", tenant) },
        project_js = if project.is_empty() { "null".to_string() } else { format!("'{}'", project) },
    );

    Html(html)
}

// ─── Handler ──────────────────────────────────────────────────────────────────

/// GET /openapi.json
pub async fn spec(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let project_id = ctx
        .project_id
        .ok_or_else(|| ApiError::bad_request("missing_project"))?;

    let (db_schema, func_values, schema_hash) =
        fetch_schema_graph_pub(&state, project_id, &headers).await?;

    let spec_json = generate_openapi(&db_schema, &func_values, &schema_hash, &state.gateway_url);

    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )],
        serde_json::to_string_pretty(&spec_json).unwrap_or_default(),
    )
        .into_response())
}

// ─── Generator ────────────────────────────────────────────────────────────────

fn generate_openapi(
    db_schema: &Value,
    functions: &[Value],
    schema_hash: &str,
    gateway_url: &str,
) -> Value {
    let tables = db_schema
        .get("tables")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    let columns = db_schema
        .get("columns")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    let mut schemas: Map<String, Value> = Map::new();
    let mut paths: Map<String, Value> = Map::new();

    // ── Per-table schemas + CRUD paths ────────────────────────────────────
    for table in tables {
        let name = match table.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let pascal = to_pascal(name);

        // Build the read schema (all columns, all optional for flexibility).
        let mut props: Map<String, Value> = Map::new();
        let mut required_fields: Vec<String> = Vec::new();

        let table_columns: Vec<&Value> = columns
            .iter()
            .filter(|c| c.get("table_name").and_then(|v| v.as_str()) == Some(name))
            .collect();

        for col in &table_columns {
            let col_name = match col.get("name").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => continue,
            };
            let fb_type = col.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            let nullable = col.get("nullable").and_then(|v| v.as_bool()).unwrap_or(true);
            let is_pk = col.get("is_primary_key").and_then(|v| v.as_bool()).unwrap_or(false);

            let schema = fb_type_to_json_schema(fb_type, nullable);
            props.insert(col_name.to_string(), schema);

            if !nullable && !is_pk {
                required_fields.push(col_name.to_string());
            }
        }

        // Full row schema (GET)
        let row_schema = json!({
            "type": "object",
            "properties": props,
            "required": required_fields,
            "x-schema-hash": schema_hash,
        });

        // Insert schema — omit server-managed columns, keep the rest required.
        let auto_cols = ["id", "created_at", "updated_at", "deleted_at"];
        let insert_props: Map<String, Value> = props
            .iter()
            .filter(|(k, _)| !auto_cols.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let insert_required: Vec<String> = required_fields
            .iter()
            .filter(|k| !auto_cols.contains(&k.as_str()))
            .cloned()
            .collect();

        let insert_schema = json!({
            "type": "object",
            "properties": insert_props,
            "required": insert_required,
        });

        schemas.insert(pascal.clone(), row_schema);
        schemas.insert(format!("{}Insert", pascal), insert_schema);

        // ── Path items ─────────────────────────────────────────────────────

        // Collection-level: GET (list) + POST (insert)
        let list_path = format!("/db/{}", name);
        let list_item = json!({
            "get": {
                "summary": format!("List {}", name),
                "tags": [name],
                "parameters": table_query_params(&pascal),
                "responses": {
                    "200": {
                        "description": "OK",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "data": {
                                            "type": "array",
                                            "items": { "$ref": format!("#/components/schemas/{}", pascal) }
                                        },
                                        "meta": { "$ref": "#/components/schemas/QueryMeta" }
                                    }
                                }
                            }
                        }
                    },
                    "400": { "$ref": "#/components/responses/BadRequest" },
                    "401": { "$ref": "#/components/responses/Unauthorized" },
                }
            },
            "post": {
                "summary": format!("Insert into {}", name),
                "tags": [name],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": format!("#/components/schemas/{}Insert", pascal) }
                        }
                    }
                },
                "responses": {
                    "201": {
                        "description": "Created",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "data": {
                                            "type": "array",
                                            "items": { "$ref": format!("#/components/schemas/{}", pascal) }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "400": { "$ref": "#/components/responses/BadRequest" },
                    "401": { "$ref": "#/components/responses/Unauthorized" },
                }
            }
        });
        paths.insert(list_path, list_item);

        // Row-level: GET (single) + PATCH (update) + DELETE
        let row_path = format!("/db/{}/{{id}}", name);
        let row_item = json!({
            "get": {
                "summary": format!("Get {} by id", name),
                "tags": [name],
                "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
                "responses": {
                    "200": {
                        "description": "OK",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "data": { "$ref": format!("#/components/schemas/{}", pascal) }
                                    }
                                }
                            }
                        }
                    },
                    "404": { "$ref": "#/components/responses/NotFound" },
                }
            },
            "patch": {
                "summary": format!("Update {} by id", name),
                "tags": [name],
                "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": format!("#/components/schemas/{}Insert", pascal) }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Updated",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "data": {
                                            "type": "array",
                                            "items": { "$ref": format!("#/components/schemas/{}", pascal) }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "404": { "$ref": "#/components/responses/NotFound" },
                }
            },
            "delete": {
                "summary": format!("Delete {} by id", name),
                "tags": [name],
                "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
                "responses": {
                    "200": {
                        "description": "Deleted",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": { "deleted": { "type": "integer" } }
                                }
                            }
                        }
                    },
                    "404": { "$ref": "#/components/responses/NotFound" },
                }
            }
        });
        paths.insert(row_path, row_item);
    }

    // ── Per-function paths ────────────────────────────────────────────────
    for func in functions {
        let fname = match func.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let description = func
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let input_schema = func
            .get("input_schema")
            .cloned()
            .unwrap_or(json!({}));
        let output_schema = func
            .get("output_schema")
            .cloned()
            .unwrap_or(json!({}));

        let pascal = to_pascal(fname);
        schemas.insert(format!("{}Input", pascal), input_schema.clone());
        schemas.insert(format!("{}Output", pascal), output_schema.clone());

        let fn_path = format!("/functions/{}", fname);
        let fn_item = json!({
            "post": {
                "summary": if description.is_empty() { format!("Invoke {}", fname) } else { description.clone() },
                "tags": ["functions"],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": format!("#/components/schemas/{}Input", pascal) }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Function result",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": format!("#/components/schemas/{}Output", pascal) }
                            }
                        }
                    },
                    "400": { "$ref": "#/components/responses/BadRequest" },
                    "401": { "$ref": "#/components/responses/Unauthorized" },
                }
            }
        });
        paths.insert(fn_path, fn_item);
    }

    // ── Shared schema components ──────────────────────────────────────────
    schemas.insert(
        "QueryMeta".to_string(),
        json!({
            "type": "object",
            "properties": {
                "rows":         { "type": "integer" },
                "elapsed_ms":   { "type": "number" },
                "complexity":   { "type": "integer" },
                "strategy":     { "type": "string" },
                "request_id":   { "type": "string" }
            }
        }),
    );

    json!({
        "openapi": "3.0.3",
        "info": {
            "title":   "Fluxbase Data API",
            "version": schema_hash.get(..8).unwrap_or(schema_hash),
            "description": "Auto-generated from live schema. Regenerate with GET /openapi.json",
            "x-schema-hash": schema_hash,
        },
        "servers": [{ "url": gateway_url, "description": "Fluxbase Gateway" }],
        "security": [{ "bearerAuth": [] }],
        "components": {
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "JWT or API Key"
                }
            },
            "schemas": schemas,
            "responses": {
                "BadRequest": {
                    "description": "Bad request",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "error": { "type": "string" },
                                    "code":  { "type": "string" }
                                }
                            }
                        }
                    }
                },
                "Unauthorized": {
                    "description": "Unauthorized",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "error": { "type": "string" }
                                }
                            }
                        }
                    }
                },
                "NotFound": {
                    "description": "Not found",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "error": { "type": "string" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "paths": paths,
    })
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn to_pascal(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

fn fb_type_to_json_schema(fb_type: &str, nullable: bool) -> Value {
    let base = match fb_type {
        "uuid" => json!({ "type": "string", "format": "uuid" }),
        "date" => json!({ "type": "string", "format": "date" }),
        "timestamp" | "timestamptz" => json!({ "type": "string", "format": "date-time" }),
        "interval" => json!({ "type": "string" }),
        "text" | "varchar" | "bpchar" | "citext" | "char" | "name" => {
            json!({ "type": "string" })
        }
        "int2" | "int4" | "int8" => json!({ "type": "integer" }),
        "float4" | "float8" | "numeric" | "money" => json!({ "type": "number" }),
        "bool" => json!({ "type": "boolean" }),
        "jsonb" | "json" => json!({}),
        "file" => json!({
            "type": "object",
            "properties": {
                "url":       { "type": "string", "format": "uri" },
                "key":       { "type": "string" },
                "size":      { "type": "integer" },
                "mime_type": { "type": "string" }
            }
        }),
        t if t.starts_with('_') => {
            let inner = fb_type_to_json_schema(&t[1..], false);
            json!({ "type": "array", "items": inner })
        }
        _ => json!({}),
    };

    if nullable {
        json!({ "oneOf": [base, { "type": "null" }] })
    } else {
        base
    }
}

fn table_query_params(pascal: &str) -> Value {
    json!([
        {
            "name": "limit",
            "in": "query",
            "schema": { "type": "integer", "default": 50 },
            "description": "Maximum rows to return"
        },
        {
            "name": "offset",
            "in": "query",
            "schema": { "type": "integer", "default": 0 },
            "description": "Rows to skip"
        },
        {
            "name": "select",
            "in": "query",
            "schema": { "type": "string" },
            "description": format!("Comma-separated column list to include in the {} response", pascal)
        },
        {
            "name": "where",
            "in": "query",
            "schema": { "type": "string" },
            "description": "URL-encoded JSON filter object"
        },
        {
            "name": "order_by",
            "in": "query",
            "schema": { "type": "string" },
            "description": "URL-encoded JSON order_by object, e.g. {\"created_at\":\"desc\"}"
        },
    ])
}
