use reqwest::{Client, StatusCode};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use lru::LruCache;
use uuid::Uuid;
use crate::config::settings::Settings;

/// Cache entry: (secrets_map, inserted_at)
type SecretsCacheEntry = (HashMap<String, String>, Instant);

/// Shared handle to the in-memory secrets cache.
///
/// Key: `"<tenant_id>/<project_id>"` or `"<tenant_id>/none"`
/// Value: resolved secrets with a TTL (default 30 s).
///
/// Because secrets rarely change between invocations, caching them eliminates
/// ~5 ms of control-plane latency on every warm execution.
#[derive(Clone)]
pub struct SecretsCache {
    inner: Arc<Mutex<LruCache<String, SecretsCacheEntry>>>,
    ttl: Duration,
}

impl SecretsCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(50).unwrap());
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(cap))),
            ttl,
        }
    }

    fn cache_key(tenant_id: Uuid, project_id: Option<Uuid>) -> String {
        match project_id {
            Some(p) => format!("{}/{}", tenant_id, p),
            None    => format!("{}/none", tenant_id),
        }
    }

    fn get(&self, key: &str) -> Option<HashMap<String, String>> {
        let mut c = self.inner.lock().unwrap();
        match c.get(key) {
            Some((secrets, inserted_at)) if inserted_at.elapsed() < self.ttl => {
                Some(secrets.clone())
            }
            Some(_) => { c.pop(key); None }  // expired
            None    => None,
        }
    }

    fn insert(&self, key: String, secrets: HashMap<String, String>) {
        let mut c = self.inner.lock().unwrap();
        c.put(key, (secrets, Instant::now()));
    }

    /// Immediately remove a cached entry (called on secret rotation).
    pub fn invalidate(&self, tenant_id: Uuid, project_id: Option<Uuid>) {
        let key = Self::cache_key(tenant_id, project_id);
        self.inner.lock().unwrap().pop(&key);
    }
}

#[derive(Clone)]
pub struct SecretsClient {
    client: Client,
    settings: Settings,
    cache: SecretsCache,
}

impl SecretsClient {
    /// Pass the shared `reqwest::Client` from `AppState` so all outbound
    /// connections share the same pooled connection set.
    pub fn new(settings: Settings, client: Client) -> Self {
        Self {
            client,
            settings,
            cache: SecretsCache::new(50, Duration::from_secs(30)),
        }
    }

    /// Expose cache handle so the invalidation endpoint can drop stale entries.
    pub fn cache(&self) -> &SecretsCache {
        &self.cache
    }

    pub async fn fetch_secrets(
        &self,
        tenant_id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<HashMap<String, String>, String> {
        let key = SecretsCache::cache_key(tenant_id, project_id);

        // Fast path: serve from in-process cache.
        if let Some(cached) = self.cache.get(&key) {
            tracing::debug!(tenant_id = %tenant_id, "secrets cache hit");
            return Ok(cached);
        }

        let mut url = format!("{}/internal/secrets?tenant_id={}", self.settings.control_plane_url, tenant_id);
        if let Some(pid) = project_id {
            url.push_str(&format!("&project_id={}", pid));
        }

        let resp = self
            .client
            .get(&url)
            .header("X-Service-Token", &self.settings.service_token)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch secrets: {}", e))?;

        let status = resp.status();
        if status != StatusCode::OK {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(format!("Control plane error HTTP {}: {}", status, error_text));
        }

        // Control plane returns ApiResponse<T>: { success: true, data: {...} }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed parsing secrets JSON: {}", e))?;

        let secrets_value = json.get("data").cloned().unwrap_or(json);

        let secrets_map: HashMap<String, String> = serde_json::from_value(secrets_value)
            .map_err(|e| format!("Failed deserializing secrets map: {}", e))?;

        self.cache.insert(key, secrets_map.clone());
        Ok(secrets_map)
    }
}
