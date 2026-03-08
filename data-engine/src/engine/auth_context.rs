use axum::http::HeaderMap;
use uuid::Uuid;

/// Auth context extracted from request headers forwarded by the gateway.
///
/// Header contract (set by gateway or caller in trusted internal network):
///   x-tenant-id:    UUID
///   x-project-id:   UUID
///   x-tenant-slug:  slug string (e.g. "acme")
///   x-project-slug: slug string (e.g. "auth")
///   x-user-id:      opaque user identifier (e.g. Firebase UID)
///   x-user-role:    role string — "anon" | "authenticated" | "admin" | "service"
#[derive(Clone, Debug)]
pub struct AuthContext {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub tenant_slug: String,
    pub project_slug: String,
    pub user_id: String,
    pub role: String,
}

impl AuthContext {
    pub fn from_headers(headers: &HeaderMap) -> Result<Self, String> {
        let tenant_id = header_uuid(headers, "x-tenant-id")
            .ok_or("missing x-tenant-id header")?;
        let project_id = header_uuid(headers, "x-project-id")
            .ok_or("missing x-project-id header")?;
        let tenant_slug = header_str(headers, "x-tenant-slug")
            .unwrap_or_else(|| tenant_id.to_string().replace('-', "_"));
        let project_slug = header_str(headers, "x-project-slug")
            .unwrap_or_else(|| project_id.to_string().replace('-', "_"));
        let user_id = header_str(headers, "x-user-id").unwrap_or_default();
        let role = header_str(headers, "x-user-role").unwrap_or_else(|| "anon".to_string());

        Ok(AuthContext {
            tenant_id,
            project_id,
            tenant_slug,
            project_slug,
            user_id,
            role,
        })
    }
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(|s| s.to_string())
}

fn header_uuid(headers: &HeaderMap, name: &str) -> Option<Uuid> {
    header_str(headers, name)?.parse().ok()
}
