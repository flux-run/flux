use job_contract::job::{CreateJobRequest, CreateJobResponse};

#[derive(Clone)]
pub struct QueueClient {
    pub base_url: String,
    pub client: reqwest::Client,
}

impl QueueClient {
    pub fn new(base_url: String, client: reqwest::Client) -> Self {
        Self { base_url, client }
    }

    pub async fn enqueue(&self, job: CreateJobRequest) -> anyhow::Result<CreateJobResponse> {
        let url = format!("{}/jobs", self.base_url.trim_end_matches('/'));

        let resp = self
            .client
            .post(url)
            .json(&job)
            .send()
            .await?
            .error_for_status()?;

        let body = resp.json::<CreateJobResponse>().await?;
        Ok(body)
    }
}
