pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Dead,
}

impl From<String> for JobStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "dead" => Self::Dead,
            _ => Self::Pending,
        }
    }
}