// Re-use everything defined in app.rs; all public items are available via `api::`
// from an external crate (lib.rs re-exports them).  main.rs stays as a thin
// entry-point that only owns the `#[tokio::main]` startup logic.

use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    api::config::init();

    let pool = api::db::connection::init_pool().await?;
    let (local_tenant_id, local_project_id) = api::app::init_local_mode(&pool).await?;

    let state = api::AppState {
        pool,
        http_client: reqwest::Client::new(),
        data_engine_url: std::env::var("DATA_ENGINE_URL")
            .unwrap_or_else(|_| "http://localhost:8082".to_string()),
        gateway_url: std::env::var("GATEWAY_URL")
            .unwrap_or_else(|_| "http://localhost:8081".to_string()),
        local_tenant_id,
        local_project_id,
        functions_dir: std::env::var("FLUX_FUNCTIONS_DIR")
            .unwrap_or_else(|_| "./flux-functions".to_string()),
    };

    let app = api::create_app(state);

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Flux API listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

