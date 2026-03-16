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

    async fn is_db_token_valid(&self, token: &str) -> Result<bool, sqlx::Error> {
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));

        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
               SELECT 1\
               FROM flux.service_tokens\
               WHERE token_hash = $1\
                 AND revoked_at IS NULL\
            )",
        )
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }

    fn read_bearer_token(metadata: &tonic::metadata::MetadataMap) -> Option<String> {
        metadata
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    async fn authenticate(
        &self,
        metadata: &tonic::metadata::MetadataMap,
    ) -> Result<String, Status> {
        let provided_token = Self::read_bearer_token(metadata).unwrap_or_default();

        if provided_token.is_empty() {
            return Err(Status::unauthenticated("missing authorization bearer token"));
        }

        if !self.expected_token.is_empty() {
            let env_match: bool = provided_token
                .as_bytes()
                .ct_eq(self.expected_token.as_bytes())
                .into();

            if env_match {
                return Ok("env".to_string());
            }
        }

        let db_match = self
            .is_db_token_valid(&provided_token)
            .await
            .map_err(|e| Status::internal(format!("token lookup failed: {e}")))?;

        if db_match {
            let token_hash = hex::encode(Sha256::digest(provided_token.as_bytes()));
            let pool = self.pool.clone();

            tokio::spawn(async move {
                let _ = sqlx::query(
                    "UPDATE flux.service_tokens\
                     SET last_used_at = now()\
                     WHERE token_hash = $1 AND revoked_at IS NULL",
                )
                .bind(token_hash)
                .execute(&pool)
                .await;
            });

            return Ok("db".to_string());
        }

        Err(Status::unauthenticated("invalid service token"))
    }
}

#[tonic::async_trait]
impl pb::internal_auth_service_server::InternalAuthService for InternalAuthGrpc {
    async fn validate_token(
        &self,
        request: Request<pb::ValidateTokenRequest>,
    ) -> Result<Response<pb::ValidateTokenResponse>, Status> {
        let auth_mode = self.authenticate(request.metadata()).await?;

        Ok(Response::new(pb::ValidateTokenResponse {
            ok: true,
            auth_mode,
        }))
    }

    async fn list_logs(
        &self,
        request: Request<pb::ListLogsRequest>,
    ) -> Result<Response<pb::ListLogsResponse>, Status> {
        let _auth_mode = self.authenticate(request.metadata()).await?;
        let _limit = request.into_inner().limit;

        Ok(Response::new(pb::ListLogsResponse { logs: vec![] }))
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
