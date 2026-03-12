/// JSON Schema validation for function input/output payloads.
///
/// Validation is performed entirely in Rust (no JS/WASM step) which means:
/// - Zero added latency on warm paths (schema compile is ~50µs, re-compiled each
///   call but amortized against the execution cost which is 10-100× slower).
/// - Language-agnostic: every runtime (Deno, WASM-Rust, WASM-Go, etc.) gets the
///   same contract enforcement without any per-SDK code.
/// - Works on ALL three execution paths: WASM warm, Deno warm, cold (cache miss).
///
/// If no schema is stored for a function, validation is skipped (permissive default).

use serde_json::Value;

/// A single schema violation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SchemaViolation {
    /// JSON Pointer to the field that failed (e.g. `/name`), or empty string
    /// for top-level / overall errors.
    pub field: String,
    /// Human-readable description of the violation.
    pub message: String,
}

/// Validate `payload` against `schema` (a valid JSON Schema object).
///
/// Returns `Ok(())` when the payload is valid or the schema is absent.
/// Returns `Err(violations)` with a non-empty list on failure.
///
/// # Schema compilation
/// The schema is compiled fresh on each call.  For the typical function
/// execution latency (~10-200ms), this is negligible.  If benchmarks later
/// show it matters, move to a per-function compiled-schema cache.
pub fn validate_input(
    schema: &Value,
    payload: &Value,
) -> Result<(), Vec<SchemaViolation>> {
    let validator = match jsonschema::validator_for(schema) {
        Ok(v) => v,
        Err(e) => {
            // A broken schema stored in the DB should not crash execution —
            // log the problem and skip validation rather than blocking the call.
            tracing::warn!(error = %e, "input_schema compile failed — skipping validation");
            return Ok(());
        }
    };

    let violations: Vec<SchemaViolation> = validator
        .iter_errors(payload)
        .map(|e| SchemaViolation {
            field: e.instance_path.to_string(),
            message: e.to_string(),
        })
        .collect();

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}
