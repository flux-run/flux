/// Structural query validator — cheap O(columns) check performed at the gateway
/// before forwarding to the data-engine.
///
/// Rejects requests whose column selectors, filter lists, or nesting depth exceed
/// platform-defined maximums.  This reduces load on the data-engine and prevents
/// accidentally (or maliciously) crafted deep-join queries from consuming DB time.
use axum::http::StatusCode;
use serde_json::Value;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct QueryGuardConfig {
    /// Maximum number of column entries allowed in a single query.  Default: 100.
    pub max_columns: usize,
    /// Maximum nesting depth of column selectors (e.g. posts → comments → likes).
    /// Default: 6.
    pub max_selector_depth: usize,
    /// Maximum number of filter expressions per query.  Default: 20.
    pub max_filters: usize,
}

impl Default for QueryGuardConfig {
    fn default() -> Self {
        Self {
            max_columns: 100,
            max_selector_depth: 6,
            max_filters: 20,
        }
    }
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate the structural complexity of a `/db/query` request body.
///
/// Returns `Ok(())` when the request is within limits, or
/// `Err((status, message))` when a limit is exceeded.
///
/// Malformed JSON is passed through to the data-engine so error messages stay
/// consistent with the data-engine's own JSON-parse errors.
pub fn validate_query_body(
    body: &[u8],
    cfg: &QueryGuardConfig,
) -> Result<(), (StatusCode, String)> {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return Ok(()), // let data-engine handle malformed JSON
    };

    // ── Column count ─────────────────────────────────────────────────────────
    if let Some(cols) = req.get("columns").and_then(|v| v.as_array()) {
        if cols.len() > cfg.max_columns {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Too many columns: {} (max {}). Reduce your column selection.",
                    cols.len(),
                    cfg.max_columns
                ),
            ));
        }

        // ── Selector depth ────────────────────────────────────────────────────
        let depth = cols
            .iter()
            .map(|v| value_depth(v, 0))
            .max()
            .unwrap_or(0);

        if depth > cfg.max_selector_depth {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Column selector is deeply nested: depth {} (max {}). \
                     Flatten the query or split into multiple requests.",
                    depth,
                    cfg.max_selector_depth
                ),
            ));
        }
    }

    // ── Filter count ─────────────────────────────────────────────────────────
    if let Some(filters) = req.get("filters").and_then(|v| v.as_array()) {
        if filters.len() > cfg.max_filters {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Too many filters: {} (max {}). Split into multiple queries.",
                    filters.len(),
                    cfg.max_filters
                ),
            ));
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compute the maximum nesting depth of arrays/objects in a JSON value.
fn value_depth(v: &Value, current: usize) -> usize {
    match v {
        Value::Array(arr) => arr
            .iter()
            .map(|child| value_depth(child, current + 1))
            .max()
            .unwrap_or(current + 1),
        Value::Object(obj) => obj
            .values()
            .map(|child| value_depth(child, current + 1))
            .max()
            .unwrap_or(current + 1),
        _ => current,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> QueryGuardConfig {
        QueryGuardConfig {
            max_columns: 5,
            max_selector_depth: 3,
            max_filters: 4,
        }
    }

    #[test]
    fn accepts_simple_query() {
        let body = br#"{"table":"users","columns":["id","name"],"filters":[]}"#;
        assert!(validate_query_body(body, &cfg()).is_ok());
    }

    #[test]
    fn rejects_too_many_columns() {
        let cols = (0..6).map(|i| format!("\"col{}\"", i)).collect::<Vec<_>>().join(",");
        let body = format!("{{\"columns\":[{}]}}", cols);
        let err = validate_query_body(body.as_bytes(), &cfg());
        assert!(err.is_err());
        assert!(err.unwrap_err().1.contains("Too many columns"));
    }

    #[test]
    fn rejects_deep_nesting() {
        // depth 4: ["posts", {"comments": [{"likes": [{"user": ["id"]}]}]}]
        let body = br#"{"columns":["posts",{"comments":[{"likes":[{"user":["id"]}]}]}]}"#;
        let err = validate_query_body(body, &cfg());
        assert!(err.is_err());
        assert!(err.unwrap_err().1.contains("deeply nested"));
    }

    #[test]
    fn rejects_too_many_filters() {
        let filters = (0..5)
            .map(|i| format!("{{\"field\":\"f{}\",\"op\":\"eq\",\"value\":{}}}", i, i))
            .collect::<Vec<_>>()
            .join(",");
        let body = format!("{{\"filters\":[{}]}}", filters);
        let err = validate_query_body(body.as_bytes(), &cfg());
        assert!(err.is_err());
        assert!(err.unwrap_err().1.contains("Too many filters"));
    }

    #[test]
    fn passes_malformed_json_through() {
        let body = b"not-json";
        assert!(validate_query_body(body, &cfg()).is_ok());
    }
}
