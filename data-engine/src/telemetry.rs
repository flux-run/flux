//! Tracing / observability initialisation.
//!
//! Separated from `config` so that loading configuration (reading env vars)
//! and initialising global subscribers (side-effects) are distinct
//! responsibilities — SRP.
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "data_engine=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}
