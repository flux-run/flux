use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
    extract::State,
};
use crate::state::SharedState;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ResolvedIdentity {
    pub tenant_id: Uuid,
    pub tenant_slug: String,
}

pub async fn resolve_identity(
    State(state): State<SharedState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1. Check direct edge tenant header (Cloudflare Worker optimization)
    let tenant_slug = if let Some(t_slug) = req.headers().get("x-tenant").and_then(|h| h.to_str().ok()) {
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

        let slug = parts[0];

        // Ignore reserved subdomains that point to platform services
        let reserved = [
            // ── Core platform ──────────────────────────────────────────
            "api", "run", "gateway", "www", "localhost",
            // ── Product surfaces ───────────────────────────────────────
            "dashboard", "console", "app", "admin", "portal",
            "demo", "sandbox", "preview", "staging", "beta", "alpha",
            // ── Developer tools ────────────────────────────────────────
            "docs", "doc", "trace", "traces", "cli", "sdk",
            "workflow", "workflows", "agents", "agent",
            "registry", "packages", "releases", "install", "download",
            // ── Auth / identity ────────────────────────────────────────
            "auth", "login", "logout", "signup", "register",
            "oauth", "sso", "saml", "id",
            // ── Observability / ops ────────────────────────────────────
            "status", "health", "ping", "metrics", "logs",
            "monitoring", "alerts", "uptime",
            // ── Infra / routing ────────────────────────────────────────
            "edge", "relay", "proxy", "ingress", "cdn",
            "static", "assets", "media", "storage", "files", "uploads",
            // ── Comms ──────────────────────────────────────────────────
            "mail", "smtp", "inbound", "outbound",
            "webhooks", "events", "hooks",
            // ── Commerce ──────────────────────────────────────────────
            "billing", "checkout", "payment", "invoices", "pricing",
            // ── Communication / community ─────────────────────────────
            "help", "support", "feedback", "community", "forum",
            "blog", "changelog", "updates", "roadmap",
            // ── Corporate / legal ─────────────────────────────────────
            "about", "careers", "jobs", "legal", "privacy", "terms",
            "security", "compliance",
            // ── Internal ──────────────────────────────────────────────
            "internal", "corp", "staff", "team", "dev",
            // ── API versioning ────────────────────────────────────────
            "v1", "v2", "v3", "v4", "grpc", "graphql",
        ];
        if reserved.contains(&slug) {
            return Err(StatusCode::NOT_FOUND);
        }
        slug.to_string()
    };

    // 2. Resolve from memory snapshot
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
