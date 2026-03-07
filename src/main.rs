mod config;
mod db;
mod middleware;
mod models;
mod routes;
mod services;
mod types;

use axum::{
    middleware as axum_middleware,
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;
use types::scope::Scope;
use firebase_auth::FirebaseAuth;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub firebase_auth: Arc<FirebaseAuth>,
}

impl axum::extract::FromRef<AppState> for sqlx::PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<FirebaseAuth> {
    fn from_ref(state: &AppState) -> Self {
        state.firebase_auth.clone()
    }
}

pub fn create_app(state: AppState) -> Router {
    // Platform Scope routes (just need valid Firebase token)
    let platform_routes = Router::new()
        .route("/auth/me", get(routes::auth::get_me))
        .route("/auth/logout", post(routes::auth::logout))
        .route("/tenants", post(routes::tenants::create_tenant))
        .route("/tenants", get(routes::tenants::get_tenants))
        .route("/tenants/{id}", get(routes::tenants::get_tenant))
        .route("/tenants/{id}", delete(routes::tenants::delete_tenant))
        .layer(axum_middleware::from_fn(|req, next| {
            middleware::scope::require_scope(Scope::Platform, req, next)
        }));

    // Tenant Scope routes (X-Fluxbase-Tenant required + membership verified)
    let tenant_routes = Router::new()
        .route("/tenants/{id}/members", get(routes::tenants::get_members))
        .route("/tenants/{id}/members", post(routes::tenants::invite_member))
        .route("/tenants/{id}/members/{user}", delete(routes::tenants::remove_member))
        .route("/projects", get(routes::projects::get_projects))
        .route("/projects", post(routes::projects::create_project))
        .route("/projects/{id}", get(routes::projects::get_project))
        .route("/projects/{id}", delete(routes::projects::delete_project))
        .layer(axum_middleware::from_fn(|req, next| {
            middleware::scope::require_scope(Scope::Tenant, req, next)
        }));

    // Project Scope routes (X-Fluxbase-Project required + project verified under tenant)
    let project_routes = Router::new()
        .route("/secrets", get(routes::secrets::list_secrets))
        .route("/secrets", post(routes::secrets::create_secret))
        .route("/secrets/{key}", delete(routes::secrets::delete_secret))
        .route("/functions", get(routes::functions::list_functions))
        .route("/functions", post(routes::functions::create_function))
        .route("/functions/{id}", get(routes::functions::get_function))
        .route("/functions/{id}", delete(routes::functions::delete_function))
        .route("/functions/{id}/deployments", get(routes::deployments::list_deployments))
        .route("/functions/{id}/deployments", post(routes::deployments::create_deployment))
        .layer(axum_middleware::from_fn(|req, next| {
            middleware::scope::require_scope(Scope::Project, req, next)
        }));

    // Combine with core authentication middleware applied to all
    Router::new()
        .merge(platform_routes)
        .merge(tenant_routes)
        .merge(project_routes)
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::context::resolve_context,
        ))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::auth::verify_auth,
        ))
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    config::init();
    let pool = db::connection::init_pool().await?;
    let firebase_project_id = std::env::var("FIREBASE_PROJECT_ID").expect("FIREBASE_PROJECT_ID required");
    let firebase_auth = Arc::new(FirebaseAuth::new(&firebase_project_id).await);
    
    let state = AppState {
        pool,
        firebase_auth,
    };
    
    let app = create_app(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    info!("Starting Fluxbase Control Plane on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use tower::ServiceExt;
    use serde_json::Value;
    use std::sync::Once;

    static INIT: Once = Once::new();

    async fn setup_app() -> (Router, sqlx::PgPool) {
        INIT.call_once(|| {
            config::init();
        });
        let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL missing");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&db_url)
            .await
            .unwrap();
            
        // We inject a mock firebase_auth during tests
        let firebase_auth = Arc::new(FirebaseAuth::new("mock-project").await);
        let state = AppState {
            pool: pool.clone(),
            firebase_auth,
        };
        (create_app(state), pool)
    }

    async fn send_request(app: Router, req: Request<Body>) -> (StatusCode, Value) {
        let response = app.oneshot(req).await.unwrap();
        let status = response.status();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json = serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));
        (status, body_json)
    }

    #[tokio::test]
    async fn test_1_login_platform_scope() {
        let (app, _) = setup_app().await;

        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/auth/me")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.get("user_id").is_some());
    }

    #[tokio::test]
    async fn test_2_create_tenant_and_verify_membership() {
        let (app, pool) = setup_app().await;

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/tenants")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .header("Content-Type", "application/json")
            .body(Body::from("{\"name\":\"Test Tenant\"}"))
            .unwrap();

        let (status, body) = send_request(app, req).await;
        assert_eq!(status, StatusCode::CREATED);
        
        let tenant_id_str = body.get("tenant_id").unwrap().as_str().unwrap();
        let tenant_id = uuid::Uuid::parse_str(tenant_id_str).unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tenant_members WHERE tenant_id = $1 AND role = 'owner'")
            .bind(tenant_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn test_3_create_project_under_tenant() {
        let (app, pool) = setup_app().await;
        let test_user_token = format!("Bearer {}", uuid::Uuid::new_v4());

        let req_tenant = Request::builder()
            .method(http::Method::POST)
            .uri("/tenants")
            .header("Authorization", &test_user_token)
            .header("Content-Type", "application/json")
            .body(Body::from("{\"name\":\"Test Corp\"}"))
            .unwrap();
        let (_, body) = send_request(app.clone(), req_tenant).await;
        let tenant_id = body.get("tenant_id").unwrap().as_str().unwrap().to_string();

        let req_proj = Request::builder()
            .method(http::Method::POST)
            .uri("/projects")
            .header("Authorization", &test_user_token)
            .header("X-Fluxbase-Tenant", &tenant_id)
            .header("Content-Type", "application/json")
            .body(Body::from("{\"name\":\"my-project\"}"))
            .unwrap();
        
        let (status, body) = send_request(app, req_proj).await;
        assert_eq!(status, StatusCode::CREATED);
        let project_id_str = body.get("project_id").unwrap().as_str().unwrap();
        let project_id = uuid::Uuid::parse_str(project_id_str).unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projects WHERE tenant_id = $1 AND id = $2")
            .bind(uuid::Uuid::parse_str(&tenant_id).unwrap())
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn test_4_tenant_route_without_tenant_header_fails() {
        let (app, _) = setup_app().await;
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/projects")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .body(Body::empty())
            .unwrap();
        let (status, _) = send_request(app, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_5_project_route_without_project_header_fails() {
        let (app, _) = setup_app().await;
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/functions")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .header("X-Fluxbase-Tenant", uuid::Uuid::new_v4().to_string())
            .body(Body::empty())
            .unwrap();
        let (status, _) = send_request(app, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_6_project_accessed_from_different_tenant() {
        let (app, _) = setup_app().await;
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/functions")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .header("X-Fluxbase-Tenant", uuid::Uuid::new_v4().to_string())
            .header("X-Fluxbase-Project", uuid::Uuid::new_v4().to_string())
            .body(Body::empty())
            .unwrap();
        let (status, _) = send_request(app, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
