use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    extract::State,
};
use crate::state::SharedState;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ResolvedIdentity {
    pub tenant_id: Uuid,
    pub tenant_slug: String,
}

// ── Reserved exact subdomains ─────────────────────────────────────────────────
// Tenant slugs may never match any of these.
const RESERVED_EXACT: &[&str] = &[
    // Core platform
    "api", "run", "gateway", "www", "localhost",
    // Product surfaces
    "dashboard", "console", "app", "admin", "portal",
    "demo", "sandbox", "preview", "staging", "beta", "alpha",
    // Developer tools
    "docs", "doc", "trace", "traces", "cli", "sdk",
    "playground", "examples", "templates", "starter", "lab", "labs", "studio",
    "workflow", "workflows", "agents", "agent",
    "registry", "packages", "install", "download",
    // AI / automation
    "ai", "llm", "model", "models", "prompt", "prompts",
    "tools", "tool", "automation", "automations",
    "assistant", "assistants",
    // Deployment / build
    "deploy", "deployments", "build", "builds",
    "release", "releases",
    // Auth / identity
    "auth", "login", "logout", "signup", "register",
    "oauth", "sso", "saml", "id", "account", "accounts",
    // Platform control
    "config", "settings", "control", "manage", "management",
    // Observability / ops
    "status", "health", "ping", "metrics", "logs",
    "monitoring", "alerts", "uptime",
    // Infra / routing
    "edge", "relay", "proxy", "ingress", "cdn",
    "static", "assets", "media", "storage", "files", "uploads",
    // Comms
    "mail", "smtp", "inbound", "outbound",
    "webhooks", "events", "hooks",
    // Commerce
    "billing", "checkout", "payment", "invoices", "pricing",
    // Community / content
    "help", "support", "feedback", "community", "forum",
    "blog", "changelog", "updates", "roadmap",
    // Corporate / legal
    "about", "careers", "jobs", "legal", "privacy", "terms",
    "security", "compliance",
    // Platform brand
    "flux", "fluxbase", "system", "core", "platform",
    // Internal
    "internal", "corp", "staff", "team", "dev",
    // API versioning
    "v1", "v2", "v3", "v4", "grpc", "graphql",
];

// ── Reserved prefix blocks ────────────────────────────────────────────────────
// Any slug that STARTS WITH one of these is also reserved.
// Prevents "api-test-org", "auth-demo-org", "flux-anything", etc.
const RESERVED_PREFIXES: &[&str] = &[
    "api", "auth", "admin", "flux", "fluxbase", "system", "core", "platform",
];

fn is_reserved(slug: &str) -> bool {
    if RESERVED_EXACT.contains(&slug) {
        return true;
    }
    RESERVED_PREFIXES.iter().any(|p| slug.starts_with(p))
}

// ── Slug normalization ────────────────────────────────────────────────────────
// - Lowercase
// - Strip non-ASCII (prevents homograph attacks, blocks xn-- punycode)
// - Keep only [a-z0-9-]
// - Collapse consecutive dashes
fn normalize_slug(raw: &str) -> Option<String> {
    let normalized: String = raw
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let collapsed = normalized
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() { None } else { Some(collapsed) }
}

fn reserved_response() -> Response {
    (
        StatusCode::MISDIRECTED_REQUEST,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        r#"{"error":"reserved","message":"This subdomain is reserved for platform use."}"#,
    )
        .into_response()
}

pub async fn resolve_identity(
    State(state): State<SharedState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1. Extract raw slug from x-tenant header (Cloudflare edge shortcut) or Host
    let raw_slug = if let Some(t_slug) = req.headers().get("x-tenant").and_then(|h| h.to_str().ok()) {
        t_slug.to_string()
    } else {
        let host = req
            .headers()
            .get("x-forwarded-host")
            .or_else(|| req.headers().get("host"))
            .and_then(|h| h.to_str().ok())
            .ok_or(StatusCode::BAD_REQUEST)?;

        let parts: Vec<&str> = host.split('.').collect();
        if parts.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        parts[0].to_string()
    };

    // 2. Normalize: lowercase + strip non-[a-z0-9-] + collapse dashes
    let tenant_slug = match normalize_slug(&raw_slug) {
        Some(s) => s,
        None => return Err(StatusCode::BAD_REQUEST),
    };

    // 3. Reserved check (exact + prefix) -- BEFORE tenant lookup
    //    Returns 421 Misdirected Request so caller knows the subdomain is
    //    intentionally claimed by the platform, not simply absent.
    if is_reserved(&tenant_slug) {
        return Ok(reserved_response());
    }

    // 4. Resolve tenant from in-memory snapshot
    let snapshot_data = state.snapshot.get_data().await;

    if let Some(&tenant_id) = snapshot_data.tenants_by_slug.get(&tenant_slug) {
        req.extensions_mut().insert(ResolvedIdentity {
            tenant_id,
            tenant_slug,
        });
        Ok(next.run(req).await)
    } else {
        tracing::debug!("Tenant not found in memory snapshot: {}", tenant_slug);
        Err(StatusCode::NOT_FOUND)
    }
}
