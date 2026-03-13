pub mod worker;

/// Configuration for the daily retention job.
/// Sourced from `data-engine/src/config.rs` at startup.
#[derive(Clone, Copy)]
pub struct RetentionConfig {
    /// Delete successful records older than this many days.
    pub record_retention_days: u32,
    /// Delete error records older than this many days.
    /// 0 = use 3× record_retention_days.
    pub error_retention_days: u32,
    /// UTC hour (0–23) to run the job each day.
    pub job_hour_utc: u32,
}

impl RetentionConfig {
    pub fn effective_error_days(&self) -> u32 {
        if self.error_retention_days > 0 {
            self.error_retention_days
        } else {
            self.record_retention_days * 3
        }
    }
}
