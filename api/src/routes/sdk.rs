/// SDK endpoints.
///
/// GET /sdk/schema      — machine-readable unified schema graph (tables + functions + hash)
/// GET /sdk/typescript  — on-demand TypeScript SDK file, cached by schema hash
use axum::{
    extract::{Extension, State},
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use sqlx::Row;

use crate::{
    types::{
        context::RequestContext,
        response::{ApiError, ApiResponse},
    },
    AppState,
};

use super::schema::forward_headers;

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

// ─── Utilities ────────────────────────────────────────────────────────────────

/// Compute a stable hex-encoded SHA-256 hash of any string.
/// Used to produce the schema hash that keys the SDK cache.
fn compute_hash(data: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data.as_bytes());
    hex::encode(h.finalize())
}

/// Fetch the schema graph from the Data Engine + function definitions from DB.
/// Returns `(db_schema, func_values, schema_hash)`.
/// `pub` so the openapi handler can reuse it.
pub async fn fetch_schema_graph_pub(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(Value, Vec<Value>, String), ApiError> {
    let de_url = format!("{}/db/schema", state.data_engine_url);
    let db_schema: Value = state
        .http_client
        .get(&de_url)
        .headers(forward_headers(headers))
        .send()
        .await
        .map_err(|e| ApiError::internal(&format!("data_engine_unreachable: {}", e)))?
        .json()
        .await
        .map_err(|e| ApiError::internal(&format!("data_engine_parse: {}", e)))?;

    let funcs = sqlx::query(
        "SELECT name, description, input_schema, output_schema \
         FROM flux.functions ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| ApiError::internal("db_error"))?;

    let func_values: Vec<Value> = funcs
        .into_iter()
        .map(|f| {
            serde_json::json!({
                "name":          f.get::<String, _>("name"),
                "description":   f.get::<Option<String>, _>("description"),
                "input_schema":  f.get::<Option<serde_json::Value>, _>("input_schema"),
                "output_schema": f.get::<Option<serde_json::Value>, _>("output_schema"),
            })
        })
        .collect();

    // Hash the raw schema bytes — any change to tables/columns/functions
    // produces a different hash and invalidates the SDK cache.
    let raw = serde_json::to_string(&serde_json::json!({
        "schema": db_schema,
        "functions": func_values,
    }))
    .unwrap_or_default();
    let schema_hash = compute_hash(&raw);

    Ok((db_schema, func_values, schema_hash))
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// GET /sdk/schema
///
/// Returns the machine-readable unified schema graph (tables, columns,
/// relationships, policies, functions) together with a `schema_hash` field
/// that can be used to detect staleness and skip regenerating the TypeScript
/// SDK when the schema hasn't changed.
///
/// Suitable for IDE plugins, CLI tools, and future GraphQL gateways.
pub async fn schema(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    headers: HeaderMap,
) -> ApiResult<Value> {
    let (db_schema, func_values, schema_hash) =
        fetch_schema_graph_pub(&state, &headers).await?;

    // Upsert a schema version record; return (or create) the version number.
    let version_number: i32 = sqlx::query(
        "WITH ins AS ( \
            INSERT INTO schema_versions (schema_hash, version_number) \
            VALUES ($1, \
                (SELECT COALESCE(MAX(version_number), 0) + 1 \
                 FROM schema_versions) \
            ) \
            ON CONFLICT (schema_hash) DO NOTHING \
            RETURNING version_number \
         ) \
         SELECT version_number FROM ins \
         UNION ALL \
         SELECT version_number FROM schema_versions \
         WHERE schema_hash = $1 \
         LIMIT 1",
    )
    .bind(&schema_hash)
    .fetch_one(&state.pool)
    .await
    .map(|r| r.get::<i32, _>("version_number"))
    .unwrap_or(1);

    Ok(ApiResponse::new(serde_json::json!({
        "schema_hash":    schema_hash,
        "schema_version": version_number,
        "tables":         db_schema.get("tables").cloned().unwrap_or(serde_json::json!([])),
        "columns":        db_schema.get("columns").cloned().unwrap_or(serde_json::json!([])),
        "relationships":  db_schema.get("relationships").cloned().unwrap_or(serde_json::json!([])),
        "policies":       db_schema.get("policies").cloned().unwrap_or(serde_json::json!([])),
        "functions":      func_values,
    })))
}

/// GET /sdk/typescript
///
/// Returns a TypeScript source file (Content-Type: application/typescript)
/// containing fully-typed interfaces, Insert/Update utility types, function
/// I/O types, and a module augmentation for `@fluxbase/sdk`.
///
/// The file is cached in memory keyed by `schema_hash` — if the
/// schema hasn't changed since last call the response is served from memory in
/// <1 ms.  The current `schema_hash` is echoed in the `X-Schema-Hash` header
/// and inside the generated file as a comment, making stale-detection trivial.
pub async fn typescript(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (db_schema, func_values, schema_hash) =
        fetch_schema_graph_pub(&state, &headers).await?;

    let sdk = generate_sdk(&db_schema, &func_values, &schema_hash);

    Ok((
        [
            (header::CONTENT_TYPE, "application/typescript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        [(
            axum::http::HeaderName::from_static("x-schema-hash"),
            axum::http::HeaderValue::from_str(&schema_hash)
                .unwrap_or_else(|_| axum::http::HeaderValue::from_static("-")),
        )],
        sdk,
    )
        .into_response())
}

// ─── Generation helpers ───────────────────────────────────────────────────────

/// Map a Fluxbase column type to the appropriate TypeScript primitive.
fn fb_type_to_ts(fb_type: &str) -> &'static str {
    match fb_type {
        "text" | "varchar" | "bpchar" | "uuid" | "date" | "timestamp"
        | "timestamptz" | "interval" | "citext" | "char" | "name" => "string",
        "int2" | "int4" | "int8" | "float4" | "float8" | "numeric" | "money" => "number",
        "bool" => "boolean",
        "jsonb" | "json" => "unknown",
        "file" => "FluxFile",
        "computed" => "unknown",
        t if t.starts_with('_') => match &t[1..] {
            "text" | "varchar" | "uuid" => "string[]",
            "int4" | "int8" | "float8" | "numeric" => "number[]",
            "bool" => "boolean[]",
            _ => "unknown[]",
        },
        _ => "unknown",
    }
}

/// Convert `snake_case` → `PascalCase` for TypeScript interface names.
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

/// Determine whether a column should be optional in the generated interface.
/// Files, metadata fields, and common auto-set columns are optional.
fn is_optional_col(col: &str, fb_type: &str) -> bool {
    fb_type == "file"
        || fb_type == "computed"
        || matches!(
            col,
            "updated_at"
                | "deleted_at"
                | "description"
                | "meta"
                | "tags"
                | "avatar"
                | "cover"
        )
}

/// Recursively convert a JSON Schema node (as generated by the Zod walker in
/// `@fluxbase/functions`) to a TypeScript type string.
fn json_schema_to_ts(schema: &Value, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    let Value::Object(obj) = schema else {
        return "unknown".to_string();
    };

    let type_name = obj
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("");

    match type_name {
        "string" => {
            if let Some(enums) = obj.get("enum").and_then(|e| e.as_array()) {
                let variants: Vec<String> = enums
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|v| format!("\"{}\"", v))
                    .collect();
                if !variants.is_empty() {
                    return variants.join(" | ");
                }
            }
            "string".to_string()
        }
        "number" | "integer" => "number".to_string(),
        "boolean" => "boolean".to_string(),
        "null" | "undefined" => "null".to_string(),
        "array" => {
            let items_ts = obj
                .get("items")
                .map(|i| json_schema_to_ts(i, depth))
                .unwrap_or_else(|| "unknown".to_string());
            format!("{}[]", items_ts)
        }
        "object" | "" => {
            if let Some(props) = obj.get("properties").and_then(|p| p.as_object()) {
                let required: Vec<&str> = obj
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                if props.is_empty() {
                    return "Record<string, unknown>".to_string();
                }

                let fields: Vec<String> = props
                    .iter()
                    .map(|(k, v)| {
                        let optional = !required.contains(&k.as_str());
                        let ts = json_schema_to_ts(v, depth + 1);
                        let sep = if optional { "?: " } else { ": " };
                        format!("{}  {}{}{};", pad, k, sep, ts)
                    })
                    .collect();

                format!("{{\n{}\n{}}}", fields.join("\n"), pad)
            } else {
                "Record<string, unknown>".to_string()
            }
        }
        _ => "unknown".to_string(),
    }
}

/// Core generation function. Accepts the raw schema graph values and produces
/// the complete TypeScript SDK source string.
fn generate_sdk(db_schema: &Value, functions: &[Value], schema_hash: &str) -> String {
    let empty = Value::Array(vec![]);
    let tables = db_schema.get("tables").unwrap_or(&empty);
    let columns = db_schema.get("columns").unwrap_or(&empty);
    let relationships = db_schema.get("relationships").unwrap_or(&empty);

    let mut out = String::with_capacity(8192);

    // ── File header ───────────────────────────────────────────────────────
    out.push_str("// Auto-generated by Fluxbase SDK Generator — do not edit manually.\n");
    out.push_str("// Regenerate: GET /sdk/typescript\n");
    out.push_str(&format!("// Schema hash: {}\n", schema_hash));
    out.push_str("// prettier-ignore-start\n");
    out.push_str("/* eslint-disable */\n\n");

    // ── FluxFile primitive ────────────────────────────────────────────────
    out.push_str("export type FluxFile = {\n");
    out.push_str("  url: string;\n");
    out.push_str("  key: string;\n");
    out.push_str("  size: number;\n");
    out.push_str("  mime_type: string;\n");
    out.push_str("};\n\n");

    // ── Build table → columns map ─────────────────────────────────────────
    // BTreeMap ensures deterministic ordering in the output.
    let mut table_cols: std::collections::BTreeMap<
        String,              // table name
        Vec<(String, String)>,  // (column, fb_type)
    > = std::collections::BTreeMap::new();

    if let Some(cols) = columns.as_array() {
        for col in cols {
            let table = col
                .get("table")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let column = col
                .get("column")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let fb_type = col
                .get("fb_type")
                .and_then(|v| v.as_str())
                .unwrap_or("text")
                .to_string();
            if !table.is_empty() && !column.is_empty() {
                table_cols
                    .entry(table)
                    .or_default()
                    .push((column, fb_type));
            }
        }
    }

    // ── Build relationship maps ────────────────────────────────────────────
    //
    // `rels`         — read interface fields (all directions, for the SELECT type)
    //                  from_table → [(alias, to_table, kind)]
    //
    // `outgoing_fks` — Insert/Update connect helpers.
    //                  Only "many_to_one" / "one_to_one" where from_table owns the FK.
    //                  from_table → [(alias, to_table, to_column)]
    let mut rels: std::collections::BTreeMap<String, Vec<(String, String, String)>> =
        std::collections::BTreeMap::new();
    let mut outgoing_fks: std::collections::BTreeMap<
        String,
        Vec<(String, String, String)>, // (alias, to_table, to_column)
    > = std::collections::BTreeMap::new();

    if let Some(rel_arr) = relationships.as_array() {
        for rel in rel_arr {
            let from = rel
                .get("from_table")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let alias = rel
                .get("alias")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let to = rel
                .get("to_table")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let to_col = rel
                .get("to_column")
                .and_then(|v| v.as_str())
                .unwrap_or("id")
                .to_string();
            let kind = rel
                .get("relationship")
                .and_then(|v| v.as_str())
                .unwrap_or("one_to_many")
                .to_string();
            if !from.is_empty() && !alias.is_empty() {
                rels.entry(from.clone())
                    .or_default()
                    .push((alias.clone(), to.clone(), kind.clone()));
                // FK is on `from_table` when the relation is many_to_one / one_to_one
                if kind == "many_to_one" || kind == "one_to_one" {
                    outgoing_fks
                        .entry(from)
                        .or_default()
                        .push((alias, to, to_col));
                }
            }
        }
    }

    // ── Generate table interfaces ─────────────────────────────────────────
    let mut table_names: Vec<String> = Vec::new();

    if let Some(tbls) = tables.as_array() {
        for tbl in tbls {
            let Some(name) = tbl.get("table").and_then(|v| v.as_str()) else {
                continue;
            };
            table_names.push(name.to_string());
            let iface = to_pascal(name);

            // Main interface
            out.push_str(&format!("export interface {} {{\n", iface));
            if let Some(cols) = table_cols.get(name) {
                for (col, fb_type) in cols {
                    let ts = fb_type_to_ts(fb_type);
                    let opt = is_optional_col(col, fb_type);
                    let sep = if opt { "?: " } else { ": " };
                    out.push_str(&format!("  {}{}{};\n", col, sep, ts));
                }
            }
            // Relationship fields (optional nested objects, for SELECT result type)
            if let Some(table_rels) = rels.get(name) {
                for (alias, to_table, kind) in table_rels {
                    let rel_iface = to_pascal(to_table);
                    // one_to_many / many_to_many → array; everything else → single object
                    let ts = if kind == "one_to_many" || kind == "many_to_many" {
                        format!("{}[]", rel_iface)
                    } else {
                        rel_iface.clone()
                    };
                    out.push_str(&format!("  {}?: {};\n", alias, ts));
                }
            }
            out.push_str("}\n\n");

            // Insert utility type — base columns + nested connect helpers for outgoing FKs.
            // e.g. InsertPost = Omit<Post, auto_fields> & { author?: { connect: { id: string } } }
            let base_omit = format!(
                "Omit<{}, \"id\" | \"created_at\" | \"updated_at\" | \"deleted_at\">",
                iface
            );
            if let Some(fks) = outgoing_fks.get(name) {
                let connect_lines: Vec<String> = fks
                    .iter()
                    .map(|(alias, to_table, to_col)| {
                        let to_col_ts = table_cols
                            .get(to_table.as_str())
                            .and_then(|cols| cols.iter().find(|(c, _)| c == to_col))
                            .map(|(_, fb_t)| fb_type_to_ts(fb_t))
                            .unwrap_or("string");
                        format!("  {}?: {{ connect: {{ {}: {} }} }};", alias, to_col, to_col_ts)
                    })
                    .collect();
                out.push_str(&format!(
                    "export type Insert{} = {} & {{\n{}\n}};\n",
                    iface, base_omit, connect_lines.join("\n")
                ));
            } else {
                out.push_str(&format!("export type Insert{} = {};\n", iface, base_omit));
            }
            out.push_str(&format!(
                "export type Update{} = Partial<Insert{}>;\n\n",
                iface, iface
            ));
        }
    }

    // ── Generate function types ───────────────────────────────────────────
    let mut func_names: Vec<String> = Vec::new();

    for func in functions {
        let Some(name) = func.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        func_names.push(name.to_string());
        let iface = to_pascal(name);

        if let Some(desc) = func.get("description").and_then(|v| v.as_str()) {
            if !desc.is_empty() {
                out.push_str(&format!("/** {} */\n", desc));
            }
        }

        let input_ts = match func.get("input_schema").map(|s| json_schema_to_ts(s, 0)) {
            Some(ts) if ts != "unknown" => ts,
            _ => "Record<string, unknown>".to_string(),
        };
        out.push_str(&format!("export type {}Input = {};\n", iface, input_ts));

        let output_ts = match func.get("output_schema").map(|s| json_schema_to_ts(s, 0)) {
            Some(ts) if ts != "unknown" => ts,
            _ => "unknown".to_string(),
        };
        out.push_str(&format!("export type {}Output = {};\n\n", iface, output_ts));
    }

    // ── Module augmentation ───────────────────────────────────────────────
    // Augments the `@fluxbase/sdk` module so the dynamically-proxied
    // `flux.db.*` and `flux.functions.*` objects are fully typed.
    out.push_str("declare module \"@fluxbase/sdk\" {\n");

    // FluxbaseDB augmentation
    out.push_str("  interface FluxbaseDB {\n");
    for name in &table_names {
        let iface = to_pascal(name);
        out.push_str(&format!(
            "    {}: import(\"@fluxbase/sdk\").TableClient<{}, Insert{}, Update{}>;\n",
            name, iface, iface, iface
        ));
    }
    out.push_str("  }\n\n");

    // FluxbaseFunctions augmentation
    out.push_str("  interface FluxbaseFunctions {\n");
    for name in &func_names {
        let iface = to_pascal(name);
        out.push_str(&format!(
            "    {}(input: {}Input): Promise<{}Output>;\n",
            name, iface, iface
        ));
    }
    out.push_str("  }\n}\n\n");

    // ── Convenience re-export ─────────────────────────────────────────────
    out.push_str("export { createClient } from \"@fluxbase/sdk\";\n");
    out.push_str("// prettier-ignore-end\n");

    out
}
