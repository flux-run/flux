use dashmap::DashMap;
use jsonwebtoken::jwk::JwkSet;
use reqwest::Client;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Clone)]
pub struct JwksCache {
    client: Client,
    cache: Arc<DashMap<String, JwkSet>>,
}

impl JwksCache {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            cache: Arc::new(DashMap::new()),
        }
    }

    pub async fn get_or_fetch(&self, url: &str) -> Option<JwkSet> {
        if let Some(jwks) = self.cache.get(url) {
            return Some(jwks.clone());
        }

        info!("Fetching JWKS from {}", url);
        match self.client.get(url).send().await {
            Ok(res) => {
                if let Ok(jwks) = res.json::<JwkSet>().await {
                    self.cache.insert(url.to_string(), jwks.clone());
                    return Some(jwks);
                } else {
                    error!("Failed to parse JWKS from {}", url);
                }
            }
            Err(e) => {
                error!("Failed to fetch JWKS from {}: {:?}", url, e);
            }
        }
        None
    }
}
