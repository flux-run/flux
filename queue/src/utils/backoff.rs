use std::time::Duration;

pub fn retry_delay(attempts: u32) -> Duration {
    Duration::from_secs(5 * (1 << attempts))
}