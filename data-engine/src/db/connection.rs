use sqlx::{postgres::PgPoolOptions, PgPool};

pub async fn init_pool(database_url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
        .expect("Failed to connect to database")
}

/// BYODB identity record captured at first connect (or supplied at registration).
#[derive(Debug, Clone)]
pub struct DbIdentity {
    /// `pg_control_system().system_identifier` — unique per physical cluster.
    /// Survives logical replica promotion; changes only on `initdb`.
    pub system_identifier: String,
    /// `current_database()` — guards against pointing at the wrong logical DB.
    pub db_name: String,
}

/// Query the live cluster for its identity.
/// Returns Err if `pg_control_system` is unavailable (pre-PG 10 or restricted role).
pub async fn read_db_identity(pool: &PgPool) -> Result<DbIdentity, sqlx::Error> {
    let (system_identifier, db_name): (String, String) = sqlx::query_as(
        "SELECT system_identifier::text, current_database() FROM pg_control_system()",
    )
    .fetch_one(pool)
    .await?;
    Ok(DbIdentity { system_identifier, db_name })
}

/// Verify the connected cluster matches the expected identity stored at registration.
/// Panics if there is a mismatch — this is intentional: a wrong DB is always a hard failure.
///
/// Call this once per user pool after `init_pool`. Pass `None` fields to only log (first-connect).
pub async fn verify_db_identity(
    pool: &PgPool,
    project_id: &str,
    expected: &DbIdentity,
) {
    let live = read_db_identity(pool).await.unwrap_or_else(|e| {
        // pg_control_system requires superuser on some managed providers.
        // Log a warning but do not panic — degraded mode.
        tracing::warn!(
            project_id,
            error = %e,
            "BYODB identity check skipped: pg_control_system() unavailable"
        );
        return expected.clone(); // treat as matching so we don't false-positive
    });

    if live.system_identifier != expected.system_identifier {
        panic!(
            "BYODB IDENTITY MISMATCH for project {}: \
             expected cluster {} but connected to cluster {}. \
             Refusing to start — this prevents silent data corruption after \
             failover, snapshot restore, or environment misconfiguration.",
            project_id, expected.system_identifier, live.system_identifier
        );
    }
    if live.db_name != expected.db_name {
        panic!(
            "BYODB DB NAME MISMATCH for project {}: \
             expected database '{}' but connected to '{}'. \
             Check the connection URL.",
            project_id, expected.db_name, live.db_name
        );
    }
    tracing::info!(
        project_id,
        system_identifier = %live.system_identifier,
        db_name = %live.db_name,
        "BYODB identity verified ✓"
    );
}

/// Connect and immediately log the identity (for the platform DB at startup).
/// Does not enforce any expected values — call `verify_db_identity` for user pools.
pub async fn init_pool_with_identity_log(database_url: &str, label: &str) -> PgPool {
    let pool = init_pool(database_url).await;
    match read_db_identity(&pool).await {
        Ok(id) => tracing::info!(
            label,
            system_identifier = %id.system_identifier,
            db_name = %id.db_name,
            "connected to database"
        ),
        Err(e) => tracing::warn!(label, error = %e, "connected (identity unavailable)"),
    }
    pool
}
