//! Log archival: moves `function_logs` rows older than `LOG_HOT_DAYS` (default 7)
//! out of Postgres and into R2/S3 as NDJSON files.
//!
//! # Object layout
//!
//! ```text
//! logs/{function_id}/{YYYY-MM-DD}/{epoch_ms}.ndjson
//! ```
//!
//! One file per (function, date, archival-run).  Using a timestamp suffix means
//! concurrent runs never collide and there is no read-modify-write needed.
//!
//! # Archival cycle
//!
//! `spawn_task` launches a background Tokio task that wakes every hour, finds
//! all rows where `timestamp < NOW() - hot_days`, groups them by
//! `(function_id, date)`, uploads each group as a compressed NDJSON file, then
//! deletes successfully uploaded rows from Postgres.
//!
//! Upload failures are non-fatal — rows stay in Postgres and will be retried on
//! the next hourly cycle.
//!
//! # Read path
//!
//! `fetch_archived` lists objects by prefix and downloads them for a given
//! function + time range.  Used by `list_project_logs` when the caller's `since`
//! timestamp falls before the hot window.

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::{Builder, Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

// ─── Core struct ─────────────────────────────────────────────────────────────

pub struct LogArchiver {
    pool:     PgPool,
    s3:       S3Client,
    bucket:   String,
    /// Number of days to keep in Postgres. Older rows are moved to object storage.
    pub hot_days: i64,
}

#[derive(sqlx::FromRow)]
struct ArchiveRow {
    id:          Uuid,
    function_id: Uuid,
    level:       String,
    message:     String,
    timestamp:   DateTime<Utc>,
}

// ─── Construction ────────────────────────────────────────────────────────────

impl LogArchiver {
    /// Build a `LogArchiver` using the same R2 / S3-compatible env vars that
    /// `StorageService` uses, but with a separate `LOG_BUCKET` target.
    ///
    /// | Variable            | Default               | Purpose                    |
    /// |---------------------|-----------------------|----------------------------|
    /// | `R2_ENDPOINT` / `S3_ENDPOINT` | `http://127.0.0.1:9000` | Object-store endpoint |
    /// | `R2_ACCESS_KEY_ID` / `S3_ACCESS_KEY_ID` | `minioadmin` |         |
    /// | `R2_SECRET_ACCESS_KEY` / `S3_SECRET_ACCESS_KEY` | `minioadmin` |  |
    /// | `LOG_BUCKET`        | `fluxbase-logs`       | Bucket for archived logs   |
    /// | `LOG_HOT_DAYS`      | `7`                   | Hot-tier retention window  |
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
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(7);

        let region_provider = RegionProviderChain::first_try(Region::new("auto"));
        let credentials = Credentials::new(access_key, secret_key, None, None, "env");

        let shared_config = aws_config::from_env()
            .region(region_provider)
            .credentials_provider(credentials)
            .endpoint_url(endpoint)
            .load()
            .await;

        let mut builder = Builder::from(&shared_config);
        builder = builder.force_path_style(true);
        let s3 = S3Client::from_conf(builder.build());

        info!(
            "LogArchiver: hot_days={}, bucket={}",
            hot_days, bucket
        );

        Arc::new(Self { pool, s3, bucket, hot_days })
    }
}

// ─── Archival write path ─────────────────────────────────────────────────────

impl LogArchiver {
    /// Archive one batch (up to 5 000 rows) of expired logs and delete them
    /// from Postgres.  Returns the number of rows successfully archived.
    pub async fn run_once(&self) -> usize {
        let cutoff = Utc::now() - chrono::Duration::days(self.hot_days);

        // Fetch the oldest expired rows — ordered so each run makes progress.
        let rows: Vec<ArchiveRow> = match sqlx::query_as(
            "SELECT id, function_id, level, message, timestamp \
             FROM function_logs \
             WHERE timestamp < $1 \
             ORDER BY function_id, timestamp \
             LIMIT 5000",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        {
            Ok(r)  => r,
            Err(e) => {
                error!("archiver: query failed: {}", e);
                return 0;
            }
        };

        if rows.is_empty() {
            return 0;
        }

        // Group by (function_id, calendar date).
        let mut groups: HashMap<(Uuid, NaiveDate), Vec<&ArchiveRow>> = HashMap::new();
        for row in &rows {
            groups
                .entry((row.function_id, row.timestamp.date_naive()))
                .or_default()
                .push(row);
        }

        let epoch_ms       = Utc::now().timestamp_millis();
        let mut archived_ids: Vec<Uuid> = Vec::with_capacity(rows.len());

        for ((function_id, date), group) in &groups {
            let key = format!(
                "logs/{}/{}/{}.ndjson",
                function_id,
                date.format("%Y-%m-%d"),
                epoch_ms
            );

            // Serialise as NDJSON (one JSON object per line).
            let mut ndjson = String::new();
            for r in group {
                let line = serde_json::json!({
                    "id":          r.id,
                    "function_id": r.function_id,
                    "level":       r.level,
                    "message":     r.message,
                    "timestamp":   r.timestamp.to_rfc3339(),
                });
                ndjson.push_str(&line.to_string());
                ndjson.push('\n');
            }

            match self.s3
                .put_object()
                .bucket(&self.bucket)
                .key(&key)
                .content_type("application/x-ndjson")
                .body(ByteStream::from(ndjson.into_bytes()))
                .send()
                .await
            {
                Ok(_) => {
                    for r in group {
                        archived_ids.push(r.id);
                    }
                    info!("archiver: uploaded {} ({} rows)", key, group.len());
                }
                Err(e) => {
                    // Upload failed — leave rows in Postgres, retry next cycle.
                    error!("archiver: upload failed for {}: {}", key, e);
                }
            }
        }

        if archived_ids.is_empty() {
            return 0;
        }

        // Delete only the rows that were successfully uploaded.
        let deleted = match sqlx::query(
            "DELETE FROM function_logs WHERE id = ANY($1)",
        )
        .bind(&archived_ids)
        .execute(&self.pool)
        .await
        {
            Ok(r)  => r.rows_affected() as usize,
            Err(e) => {
                error!("archiver: delete failed (rows will be re-archived next cycle): {}", e);
                0
            }
        };

        info!(
            "archiver: cycle complete — {} uploaded, {} deleted from postgres",
            archived_ids.len(),
            deleted
        );
        deleted
    }

    /// Spawn a background Tokio task that runs [`run_once`] every hour.
    ///
    /// The first run is delayed 5 minutes so the service has time to warm up
    /// before hitting Postgres and R2.
    pub fn spawn_task(self: Arc<Self>) {
        tokio::spawn(async move {
            // Short grace period on startup.
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;

            let mut tick = tokio::time::interval(
                std::time::Duration::from_secs(3_600), // 1 hour
            );
            loop {
                tick.tick().await;
                let n = self.run_once().await;
                if n > 0 {
                    info!("archiver: archived {} total log rows this cycle", n);
                }
            }
        });
    }
}

// ─── Archive read path ───────────────────────────────────────────────────────

impl LogArchiver {
    /// Fetch archived log entries for a single function between two timestamps.
    ///
    /// Iterates day-by-day over the date range, lists all archive objects under
    /// `logs/{function_id}/{date}/`, downloads and parses each NDJSON file, and
    /// returns rows that fall within `[from, to]`.
    ///
    /// Results are sorted by timestamp ascending and capped at `limit`.
    pub async fn fetch_archived(
        &self,
        function_id: Uuid,
        from:        DateTime<Utc>,
        to:          DateTime<Utc>,
        limit:       usize,
    ) -> Vec<serde_json::Value> {
        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut date    = from.date_naive();
        let to_date     = to.date_naive();

        while date <= to_date {
            if results.len() >= limit {
                break;
            }

            let prefix = format!(
                "logs/{}/{}/",
                function_id,
                date.format("%Y-%m-%d")
            );

            let objects = match self.s3
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix)
                .send()
                .await
            {
                Ok(resp) => resp.contents.unwrap_or_default(),
                Err(_)   => {
                    date = date.succ_opt().unwrap_or(date);
                    continue;
                }
            };

            for obj in objects {
                if results.len() >= limit {
                    break;
                }
                let Some(key) = obj.key else { continue };

                let output = match self.s3
                    .get_object()
                    .bucket(&self.bucket)
                    .key(&key)
                    .send()
                    .await
                {
                    Ok(o)  => o,
                    Err(_) => continue,
                };

                let bytes = match output.body.collect().await {
                    Ok(b)  => b.into_bytes(),
                    Err(_) => continue,
                };

                let text = String::from_utf8_lossy(&bytes);
                for line in text.lines() {
                    if results.len() >= limit {
                        break;
                    }
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                        continue;
                    };
                    if let Some(ts_str) = v.get("timestamp").and_then(|t| t.as_str()) {
                        if let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) {
                            let ts_utc = ts.with_timezone(&Utc);
                            if ts_utc >= from && ts_utc <= to {
                                results.push(v);
                            }
                        }
                    }
                }
            }

            date = date.succ_opt().unwrap_or(date);
        }

        // Stable ascending sort by timestamp string (ISO 8601 sorts lexically).
        results.sort_by(|a, b| {
            a.get("timestamp")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .cmp(
                    b.get("timestamp")
                        .and_then(|t| t.as_str())
                        .unwrap_or(""),
                )
        });

        results
    }
}
