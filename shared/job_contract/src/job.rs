use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJobRequest {
    pub function_id: Uuid,
    pub payload: Value,
    /// Optional caller-supplied deduplication key.
    /// If a job with this key already exists, the existing job_id is returned
    /// rather than creating a duplicate.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// Delay before the job becomes eligible to run.
    /// e.g. 300 = run no sooner than 5 minutes from now.
    /// Omit (or pass null/0) for immediate execution.
    #[serde(default)]
    pub delay_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJobResponse {
    pub job_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    // ── CreateJobRequest ──────────────────────────────────────────────────

    #[test]
    fn create_job_request_roundtrip() {
        let fid = Uuid::new_v4();
        let req = CreateJobRequest {
            function_id:     fid,
            payload:         json!({"task": "send_email"}),
            idempotency_key: Some("idem-key-1".to_string()),
            delay_seconds:   Some(60),
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let back: CreateJobRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.function_id,     fid);
        assert_eq!(back.idempotency_key, Some("idem-key-1".to_string()));
        assert_eq!(back.delay_seconds,   Some(60));
    }

    #[test]
    fn create_job_request_defaults_optional_fields() {
        let json_str = format!(
            r#"{{"function_id":"{}","payload":{{}}}}"#,
            Uuid::new_v4()
        );
        let req: CreateJobRequest = serde_json::from_str(&json_str).unwrap();
        assert!(req.idempotency_key.is_none());
        assert!(req.delay_seconds.is_none());
    }

    #[test]
    fn create_job_request_clone() {
        let req = CreateJobRequest {
            function_id:     Uuid::new_v4(),
            payload:         json!({}),
            idempotency_key: None,
            delay_seconds:   None,
        };
        let c = req.clone();
        assert_eq!(c.function_id, req.function_id);
    }

    // ── CreateJobResponse ─────────────────────────────────────────────────

    #[test]
    fn create_job_response_roundtrip() {
        let id = Uuid::new_v4();
        let resp = CreateJobResponse { job_id: id };
        let json_str = serde_json::to_string(&resp).unwrap();
        let back: CreateJobResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.job_id, id);
    }

    #[test]
    fn create_job_response_from_json() {
        let id = Uuid::new_v4();
        let json_str = format!(r#"{{"job_id":"{}"}}"#, id);
        let resp: CreateJobResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(resp.job_id, id);
    }

    #[test]
    fn zero_delay_seconds_is_valid() {
        let req = CreateJobRequest {
            function_id:     Uuid::new_v4(),
            payload:         json!({}),
            idempotency_key: None,
            delay_seconds:   Some(0),
        };
        assert_eq!(req.delay_seconds, Some(0));
    }
}
