use uuid::Uuid;

/// Request-scoped context injected by `middleware::auth::require_auth`.
///
/// In standalone local mode every request gets:
///   `project_id` — from `X-Flux-Project` header, falling back to
///                   `AppState::local_project_id` (the single local project).
///   `tenant_id`  — always `AppState::local_tenant_id`; the FK anchor
///                   required by functions/secrets/deployments tables in the
///                   existing schema.  In standalone mode there is only ONE
///                   tenant — the local one.
///
/// When `FLUX_API_KEY` is set the middleware also validates the Bearer token
/// before injecting this context.
#[derive(Clone, Debug)]
pub struct RequestContext {
    /// Scopes function/secret/deployment reads and writes.
    pub project_id: Uuid,
    /// FK anchor for legacy multi-tenancy columns — always the local tenant.
    pub tenant_id:  Uuid,
}
