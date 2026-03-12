use axum::http::HeaderMap;

/// Auth context extracted from request headers forwarded by the gateway.
///
/// Flux is a single-project framework — there is no multi-tenant concept.
///
/// Header contract (set by gateway or caller in trusted internal network):
///   x-user-id:     opaque user identifier (e.g. Firebase UID)
///   x-user-role:   role string — "anon" | "authenticated" | "admin" | "service"
///   x-flux-replay: "true" — replay mode: skip hooks, events, workflows (data only)
#[derive(Clone, Debug)]
pub struct AuthContext {
    pub user_id: String,
    pub role: String,
    /// When true the caller is replaying past mutations to rebuild state.
    /// Side-effect triggers (hooks, events, workflows) are suppressed so
    /// replay does not resend emails, fire webhooks, or start new workflows.
    pub is_replay: bool,
}

impl AuthContext {
    pub fn from_headers(headers: &HeaderMap) -> Result<Self, String> {
        let user_id = header_str(headers, "x-user-id").unwrap_or_default();
        let role = header_str(headers, "x-user-role").unwrap_or_else(|| "anon".to_string());
        let is_replay = headers
            .get("x-flux-replay")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        Ok(AuthContext { user_id, role, is_replay })
    }
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(|s| s.to_string())
}
