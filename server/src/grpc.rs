use std::net::SocketAddr;

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use subtle::ConstantTimeEq;
use tokio::sync::watch;
use tonic::{Request, Response, Status};

pub mod pb {
    tonic::include_proto!("flux.internal.v1");
}

#[derive(Clone)]
pub struct InternalAuthGrpc {
    pool: PgPool,
    expected_token: String,
}

impl InternalAuthGrpc {
    pub fn new(pool: PgPool, expected_token: String) -> Self {
        Self { pool, expected_token }
    }

    async fn is_db_token_valid(&self, service_name: &str, token: &str) -> Result<bool, sqlx::Error> {
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));

        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
               SELECT 1\
               FROM flux.service_tokens\
               WHERE token_hash = $1\
                 AND revoked_at IS NULL\
                 AND (service_name = $2 OR service_name = '*')\
            )",
        )
        .bind(token_hash)
        .bind(service_name)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }
}

#[tonic::async_trait]
impl pb::internal_auth_service_server::InternalAuthService for InternalAuthGrpc {
    async fn validate_service_token(
        &self,
        request: Request<pb::ValidateServiceTokenRequest>,
    ) -> Result<Response<pb::ValidateServiceTokenResponse>, Status> {
        let provided_token = request
            .metadata()
            .get("x-service-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let service_name = request.into_inner().service_name;
        if service_name.trim().is_empty() {
            return Err(Status::invalid_argument("service_name is required"));
        }

        if provided_token.is_empty() {
            return Err(Status::unauthenticated("missing x-service-token metadata"));
        }

        let env_match: bool = provided_token
            .as_bytes()
            .ct_eq(self.expected_token.as_bytes())
            .into();

        if env_match {
            return Ok(Response::new(pb::ValidateServiceTokenResponse {
                ok: true,
                auth_mode: "env".to_string(),
            }));
        }

        let db_match = self
            .is_db_token_valid(&service_name, &provided_token)
            .await
            .map_err(|e| Status::internal(format!("token lookup failed: {e}")))?;

        if db_match {
            let token_hash = hex::encode(Sha256::digest(provided_token.as_bytes()));
            let pool = self.pool.clone();
            let svc = service_name.clone();

            tokio::spawn(async move {
                let _ = sqlx::query(
                    "UPDATE flux.service_tokens\
                     SET last_used_at = now()\
                     WHERE token_hash = $1 AND service_name = $2 AND revoked_at IS NULL",
                )
                .bind(token_hash)
                .bind(svc)
                .execute(&pool)
                .await;
            });

            return Ok(Response::new(pb::ValidateServiceTokenResponse {
                ok: true,
                auth_mode: "db".to_string(),
            }));
        }

        Err(Status::unauthenticated("invalid service token"))
    }
}

pub async fn serve(
    addr: SocketAddr,
    service: InternalAuthGrpc,
    mut shutdown_rx: watch::Receiver<()>,
) -> Result<(), tonic::transport::Error> {
    tonic::transport::Server::builder()
        .add_service(pb::internal_auth_service_server::InternalAuthServiceServer::new(service))
        .serve_with_shutdown(addr, async move {
            let _ = shutdown_rx.changed().await;
        })
        .await
}
