//! Platform-wide unified log archival.
//!
//! # Object layout
//!
//! ```text
//! logs/{tenant_id}/{YYYY}/{MM}/{DD}/{source}/{resource_id}/{HH}-{epoch_ms}.ndjson.gz
//! ```
//!
//! Rows are grouped by `(tenant_id, source, resource_id, date, hour)`.  One
//! `.ndjson.gz` file per group per archival run.  Gzip gives 5-10× compression
//! on typical log text, keeping R2 costs negligible.
//!
//! # Archival cycle
//!
//! `spawn_task` wakes every hour, batches up to 5 000 expired rows from
//! `platform_logs` (where `timestamp < NOW() - LOG_HOT_DAYS`), uploads each
//! group as a gzip-compressed NDJSON file, then deletes only the rows that
//! were successfully uploaded.  Upload failures are retried next cycle.
//!
//! # Read path
//!
//! `fetch_archived` lists objects under a tenant/source/resource prefix, decompresses
//! each file, and returns rows within the requested timestamp window.  Used by
//! `list_project_logs` when `since` reaches back past the hot window.

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::{Builder, Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use flate2::{write::GzEncoder, Compression};
use sqlx::PgPool;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

// ─── Core struct ─────────────────────────────────────────────────────────────

pub struct LogArchiver {
    pool:         PgPool,
    s3:           S3Client,
    bucket:       String,
    /// Number of days to keep in Postgres hot tier.
    pub hot_days: i64,
    /// Max rows fetched per archival cycle.
    pub batch:    usize,
}

/// Row shape read from `platform_logs` during archival.
#[derive(sqlx::FromRow)]
struct ArchiveRow {
    id:          Uuid,
    tenant_id:   Uuid,
    project_id:  Option<Uuid>,
    source:      String,
    resource_id: String,
    level:       String,
    message:     String,
    request_id:  Option<String>,
    metadata:    Option<serde_json::Value>,
    timestamp:   DateTime<Utc>,
}

/// Grouping key for a single archive file.
/// One gzip file per (tenant, source, resource, date, hour) per cycle.
#[derive(Hash, Eq, PartialEq)]
struct GroupKey {
    tenant_id:   Uuid,
    source:      String,
    resource_id: String,
    date:        NaiveDate,
    hour:        u32,
}

// ─── Construction ────────────────────────────────────────────────────────────

impl LogArchiver {
    /// Build a `LogArchiver` from env vars.
    ///
    /// | Variable                          | Default               |
    /// |-----------------------------------|-----------------------|
    /// | `R2_ENDPOINT` / `S3_ENDPOINT`     | `http://127.0.0.1:9000` |
    /// | `R2_ACCESS_KEY_ID` / `S3_ACCESS_KEY_ID` | `minioadmin`   |
    /// | `R2_SECRET_ACCESS_KEY` / `S3_SECRET_ACCESS_KEY` | `minioadmin` |
    /// | `LOG_BUCKET`                      | `fluxbase-logs`       |
    /// | `LOG_HOT_DAYS`                    | `7`                   |
    /// | `LOG_ARCHIVE_BATCH`               | `5000`                |
    pub async fn new(pool: PgPool) -> Arc<Self> {
        let endpoint = std::env::var("R2_ENDPOINT")
            .or_else(|_| std::env::var("S3_ENDPOINT"))
            .unwrap_or_else(|_| "http://127.0.0.1:9000".to_string());

        let access_key = std::env::var("R2_ACCESS_KEY_ID")
            .or_else(|_| std::env::var("S3_ACCESS_KEY_ID"))
            .unwrap_or_else(|_| "minioadmin".to_string());

        let secret_key = std::env::var("R2_SECRET_ACCESS_KEY")
            .or_else(|_| std::env::var("S3_SECRET_ACCESS_KEY"))
            .unwrap_or_else(|_| "minioadmin".to_string());

        let bucket = std::env::var("LOG_BUCKET")
            .unwrap_or_else(|_| "fluxbase-logs".to_string());

        let hot_days = std::env::var("LOG_HOT_DAYS")
            .ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(7);

        let batch = std::env::var("LOG_ARCHIVE_BATCH")
            .ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(5_000);

        let region_provider = RegionProviderChain::first_try(Region::new("auto"));
        let credentials     = Credentials::new(access_key, secret_key, None, None, "env");

        let shared_config = aws_config::from_env()
            .region(region_provider)
            .credentials_provider(credentials)
            .endpoint_url(endpoint)
            .load().await;

        let mut builder = Builder::from(&shared_config);
        builder = builder.force_path_style(true);
        let s3 = S3Client::from_conf(builder.build());

        info!("LogArchiver: hot_days={hot_days}, batch={batch}, bucket={bucket}");
        Arc::new(Self { pool, s3, bucket, hot_days, batch })
    }
}

// ─── Archival write path ─────────────────────────────────────────────────────

impl LogArchiver {
    /// Archive one batch of expired rows.  Returns rows successfully archived.
    pub async fn run_once(&self) -> usize {
        let cutoff = Utc::now() - chrono::Duration::days(self.hot_days);

        let rows: Vec<ArchiveRow> = match sqlx::query_as(
            "SELECT id, tenant_id, project_id, source, resource_id, \
                    level, message, request_id, metadata, timestamp \
             FROM platform_logs \
             WHERE timestamp < $1 \
             ORDER BY tenant_id, source, resource_id, timestamp \
             LIMIT $2",
        )
        .bind(cutoff)
        .bind(self.batch as i64)
        .fetch_all(&self.pool).await
        {
            Ok(r)  => r,
            Err(e) => { error!("archiver: query failed: {e}"); return 0; }
        };

        if rows.is_empty() { return 0; }

        // Group by (tenant_id, source, resource_id, date, hour).
        let mut groups: HashMap<GroupKey, Vec<&ArchiveRow>> = HashMap::new();
        for row in &rows {
            groups.entry(GroupKey {
                tenant_id:   row.tenant_id,
                source:      row.source.clone(),
                resource_id: row.resource_id.clone(),
                date:        row.timestamp.date_naive(),
                hour:        row.timestamp.hour(),
            }).or_default().push(row);
        }

        let epoch_ms = Utc::now().timestamp_millis();
        let mut archived_ids: Vec<Uuid> = Vec::with_capacity(rows.len());

        for (key, group) in &groups {
            // Object key: logs/{tenant}/{YYYY}/{MM}/{DD}/{source}/{resource}/{HH}-{ts}.ndjson.gz
            let resource_slug = if key.resource_id.is_empty() {
                "_".to_string()
            } else {
                key.resource_id.replace(['/', '\\', ' '], "_")
            };
            let s3_key = format!(
                "logs/{}/{}/{}/{}/{}/{}/{:02}-{}.ndjson.gz",
                key.tenant_id,
                key.date.year(),
                key.date.month(),
                key.date.day(),
                key.source,
                resource_slug,
                key.hour,
                epoch_ms,
            );

            // Build NDJSON in memory then gzip-compress.
            let mut ndjson = String::with_capacity(group.len() * 180);
            for r in group {
                let line = serde_json::json!({
                    "id":          r.id,
                    "tenant_id":   r.tenant_id,
                    "project_id":  r.project_id,
                    "source":      r.source,
                    "resource_id": r.resource_id,
                    "level":       r.level,
                    "message":     r.message,
                    "request_id":  r.request_id,
                    "metadata":    r.metadata,
                    "timestamp":   r.timestamp.to_rfc3339(),
                });
                ndjson.push_str(&line.to_string());
                ndjson.push('\n');
            }

            let compressed = {
                let mut enc = GzEncoder::new(Vec::new(), Compression::default());
                if enc.write_all(ndjson.as_bytes()).is_err() {
                    error!("archiver: gzip encode failed for {s3_key}");
                    continue;
                }
                match enc.finish() {
                    Ok(b) => b,
                    Err(e) => { error!("archiver: gzip finish failed: {e}"); continue; }
                }
            };

            match self.s3
                .put_object()
                .bucket(&self.bucket)
                .key(&s3_key)
                .content_type("application/x-ndjson")
                .content_encoding("gzip")
                .body(ByteStream::from(compressed))
                .send().await
            {
                Ok(_) => {
                    for r in group { archived_ids.push(r.id); }
                    info!("archiver: uploaded {s3_key} ({} rows)", group.len());
                }
                Err(e) => {
                    error!("archiver: upload failed for {s3_key}: {e}");
                }
            }
        }

        if archived_ids.is_empty() { return 0; }

        let deleted = match sqlx::query(
            "DELETE FROM platform_logs WHERE id = ANY($1)",
        )
        .bind(&archived_ids)
        .execute(&self.pool).await
        {
            Ok(r)  => r.rows_affected() as usize,
            Err(e) => {
                error!("archiver: delete failed (retry next cycle): {e}");
                0
            }
        };

        info!(
            "archiver: cycle complete — {} uploaded, {} deleted from postgres",
            archived_ids.len(), deleted
        );
        deleted
    }

    /// Spawn a background Tokio task — wakes every hour, first run after 5 min.
    pub fn spawn_task(self: Arc<Self>) {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(3_600));
            loop {
                tick.tick().await;
                let n = self.run_once().await;
                if n > 0 { info!("archiver: archived {n} rows this cycle"); }
            }
        });
    }
}

// ─── Archive read path ───────────────────────────────────────────────────────

impl LogArchiver {
    /// Fetch archived log entries for a given tenant, source, and resource between
    /// two timestamps.  Walks day-by-day, lists `.ndjson.gz` objects, decompresses
    /// and filters by timestamp.  Results are ascending by timestamp, capped at `limit`.
    ///
    /// `resource_id` may be empty — if so, all resources for the source are scanned.
    pub async fn fetch_archived(
        &self,
        tenant_id:   Uuid,
        source:      &str,
        resource_id: &str,   // empty = all resources
        from:        DateTime<Utc>,
        to:          DateTime<Utc>,
        limit:       usize,
    ) -> Vec<serde_json::Value> {
        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut date    = from.date_naive();
        let to_date     = to.date_naive();

        while date <= to_date && results.len() < limit {
            // Prefix up to source level (or source/resource if known).
            let prefix = if resource_id.is_empty() {
                format!(
                    "logs/{}/{}/{}/{}/{}/",
                    tenant_id, date.year(), date.month(), date.day(), source
                )
            } else {
                let slug = resource_id.replace(['/', '\\', ' '], "_");
                format!(
                    "logs/{}/{}/{}/{}/{}/{}/",
                    tenant_id, date.year(), date.month(), date.day(), source, slug
                )
            };

            let objects = match self.s3
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix)
                .send().await
            {
                Ok(r) => r.contents.unwrap_or_default(),
                Err(_) => { date = date.succ_opt().unwrap_or(date); continue; }
            };

            'obj: for obj in objects {
                if results.len() >= limit { break 'obj; }
                let Some(key) = obj.key else { continue };

                let output = match self.s3.get_object().bucket(&self.bucket).key(&key).send().await {
                    Ok(o)  => o,
                    Err(_) => continue,
                };
                let raw = match output.body.collect().await {
                    Ok(b) => b.into_bytes(),
                    Err(_) => continue,
                };

                // Decompress gzip (objects from old format stored without gzip — fall back).
                let text = if key.ends_with(".gz") {
                    use flate2::read::GzDecoder;
                    use std::io::Read;
                    let mut dec = GzDecoder::new(&raw[..]);
                    let mut out = String::new();
                    if dec.read_to_string(&mut out).is_err() { continue; }
                    out
                } else {
                    String::from_utf8_lossy(&raw).into_owned()
                };

                for line in text.lines() {
                    if results.len() >= limit { break; }
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
                    let Some(ts_str) = v.get("timestamp").and_then(|t| t.as_str()) else { continue };
                    let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) else { continue };
                    let ts_utc = ts.with_timezone(&Utc);
                    if ts_utc >= from && ts_utc <= to {
                        results.push(v);
                    }
                }
            }

            date = date.succ_opt().unwrap_or(date);
        }

        // Stable ascending sort (ISO 8601 is lexically ordered).
        results.sort_by(|a, b| {
            a.get("timestamp").and_then(|t| t.as_str()).unwrap_or("")
                .cmp(b.get("timestamp").and_then(|t| t.as_str()).unwrap_or(""))
        });
        results
    }
}
