//! Input validation helpers shared across API route handlers.

use serde::Deserialize;

// ── Pagination ────────────────────────────────────────────────────────────────

/// Standard pagination query parameters for list endpoints.
///
/// `?limit=50&offset=0`
///
/// - `limit`: max rows to return, capped at 200 (default 50)
/// - `offset`: 0-based row offset (default 0)
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 { 50 }

impl PaginationQuery {
    /// Return (limit, offset) clamped to safe values.
    pub fn clamped(&self) -> (i64, i64) {
        let limit  = self.limit.clamp(1, 200);
        let offset = self.offset.max(0);
        (limit, offset)
    }
}

// ── Name validation ───────────────────────────────────────────────────────────

/// Validate a resource name (function, queue, agent, etc.).
///
/// Rules:
/// - 1–64 characters
/// - Only alphanumeric, `-`, `_`
/// - No path-traversal characters (`/`, `\`, `..`, `~`, null byte)
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name must not be empty".to_string());
    }
    if name.len() > 64 {
        return Err(format!("name must not exceed 64 characters (got {})", name.len()));
    }
    for ch in name.chars() {
        if !ch.is_alphanumeric() && ch != '-' && ch != '_' {
            return Err(format!("name contains invalid character: {:?}", ch));
        }
    }
    Ok(())
}

/// Validate a route path supplied by the user.
///
/// Rules:
/// - 1–512 characters
/// - Must start with `/`
/// - No `..` path traversal segments
/// - No null bytes
/// - No backslashes
pub fn validate_route_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("route path must not be empty".to_string());
    }
    if path.len() > 512 {
        return Err(format!("route path must not exceed 512 characters (got {})", path.len()));
    }
    if !path.starts_with('/') {
        return Err("route path must start with '/'".to_string());
    }
    if path.contains('\0') {
        return Err("route path must not contain null bytes".to_string());
    }
    if path.contains('\\') {
        return Err("route path must not contain backslashes".to_string());
    }
    for segment in path.split('/') {
        if segment == ".." || segment == "." {
            return Err("route path must not contain '.' or '..' segments".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PaginationQuery ───────────────────────────────────────────────────────

    #[test]
    fn pagination_defaults() {
        let q = PaginationQuery { limit: default_limit(), offset: 0 };
        assert_eq!(q.clamped(), (50, 0));
    }

    #[test]
    fn pagination_clamps_limit_to_max() {
        let q = PaginationQuery { limit: 9999, offset: 0 };
        assert_eq!(q.clamped().0, 200);
    }

    #[test]
    fn pagination_clamps_limit_to_min() {
        let q = PaginationQuery { limit: 0, offset: 0 };
        assert_eq!(q.clamped().0, 1);
    }

    #[test]
    fn pagination_clamps_negative_offset() {
        let q = PaginationQuery { limit: 10, offset: -5 };
        assert_eq!(q.clamped().1, 0);
    }

    #[test]
    fn pagination_valid_values_pass_through() {
        let q = PaginationQuery { limit: 100, offset: 200 };
        assert_eq!(q.clamped(), (100, 200));
    }

    // ── validate_name ─────────────────────────────────────────────────────────

    #[test]
    fn name_valid_alphanumeric_dash_underscore() {
        assert!(validate_name("hello-world_123").is_ok());
        assert!(validate_name("my_function").is_ok());
        assert!(validate_name("abc").is_ok());
    }

    #[test]
    fn name_empty_rejected() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn name_too_long_rejected() {
        let long = "a".repeat(65);
        assert!(validate_name(&long).is_err());
    }

    #[test]
    fn name_max_length_allowed() {
        let exactly_64 = "a".repeat(64);
        assert!(validate_name(&exactly_64).is_ok());
    }

    #[test]
    fn name_slash_rejected() {
        assert!(validate_name("my/function").is_err());
    }

    #[test]
    fn name_dotdot_rejected() {
        assert!(validate_name("..").is_err());
    }

    #[test]
    fn name_tilde_rejected() {
        assert!(validate_name("~admin").is_err());
    }

    #[test]
    fn name_null_byte_rejected() {
        assert!(validate_name("bad\0name").is_err());
    }

    #[test]
    fn name_space_rejected() {
        assert!(validate_name("my function").is_err());
    }

    // ── validate_route_path ───────────────────────────────────────────────────

    #[test]
    fn route_path_valid() {
        assert!(validate_route_path("/api/v1/users").is_ok());
        assert!(validate_route_path("/").is_ok());
        assert!(validate_route_path("/health-check").is_ok());
    }

    #[test]
    fn route_path_must_start_with_slash() {
        assert!(validate_route_path("api/users").is_err());
    }

    #[test]
    fn route_path_dotdot_rejected() {
        assert!(validate_route_path("/../../etc/passwd").is_err());
        assert!(validate_route_path("/api/../../../secret").is_err());
    }

    #[test]
    fn route_path_single_dot_rejected() {
        assert!(validate_route_path("/./secret").is_err());
    }

    #[test]
    fn route_path_null_byte_rejected() {
        assert!(validate_route_path("/api/\0evil").is_err());
    }

    #[test]
    fn route_path_backslash_rejected() {
        assert!(validate_route_path("/api\\evil").is_err());
    }

    #[test]
    fn route_path_too_long_rejected() {
        let long = format!("/{}", "a".repeat(512));
        assert!(validate_route_path(&long).is_err());
    }

    #[test]
    fn route_path_max_length_allowed() {
        let exactly_512 = format!("/{}", "a".repeat(511));
        assert!(validate_route_path(&exactly_512).is_ok());
    }

    #[test]
    fn route_path_empty_rejected() {
        assert!(validate_route_path("").is_err());
    }
}
