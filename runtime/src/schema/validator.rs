/// JSON Schema validation for function input payloads.
///
/// Performed entirely in Rust (~50 µs).  Language-agnostic — all runtime paths
/// (Deno, WASM-Rust, WASM-Go) get the same contract enforcement.
///
/// If no schema is stored for a function, validation is skipped (permissive default).
use serde_json::Value;

/// A single schema violation returned to the caller on `INPUT_VALIDATION_ERROR`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SchemaViolation {
    /// JSON Pointer to the failing field (e.g. `/name`), or empty for top-level errors.
    pub field:   String,
    pub message: String,
}

/// Validate `payload` against `schema`.
///
/// Returns `Ok(())` when valid. Returns `Err(violations)` with a non-empty list on failure.
pub fn validate_input(
    schema:  &Value,
    payload: &Value,
) -> Result<(), Vec<SchemaViolation>> {
    let validator = match jsonschema::validator_for(schema) {
        Ok(v)  => v,
        Err(e) => {
            // A broken schema in the DB must not block execution — skip and warn.
            tracing::warn!(error = %e, "input_schema compile failed — skipping validation");
            return Ok(());
        }
    };

    let violations: Vec<SchemaViolation> = validator
        .iter_errors(payload)
        .map(|e| SchemaViolation { field: e.instance_path.to_string(), message: e.to_string() })
        .collect();

    if violations.is_empty() { Ok(()) } else { Err(violations) }
}
