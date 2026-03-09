/// OpenAPI 3.0 generator.
///
/// GET /openapi.json
///
/// Generates a complete OpenAPI 3.0 specification from the live schema graph
/// (tables, columns, functions) for the current project. This is suitable for
/// importing into Postman, Insomnia, Swagger UI, or code generator tooling.
use axum::extract::{Extension, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Map, Value};

use crate::{
    types::{
        context::RequestContext,
        response::ApiError,
    },
    AppState,
};

use super::sdk::fetch_schema_graph_pub;

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
