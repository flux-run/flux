/// Request context injected by `middleware::auth::require_auth`.
///
/// Validates the bearer token (if `FLUX_API_KEY` is set) before letting
/// the request through. The system is single-tenant — no project or
/// tenant scoping is needed.
#[derive(Clone, Debug)]
pub struct RequestContext;
