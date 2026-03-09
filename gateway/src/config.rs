use std::env;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub runtime_url: String,
    pub queue_url: String,
    pub data_engine_url: String,
    pub internal_service_token: String,
    pub port: u16,
}

impl Config {
    pub fn load() -> Self {
        Self {
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL required"),
            runtime_url: env::var("RUNTIME_URL").unwrap_or_else(|_| "http://localhost:3001".to_string()),
            queue_url: env::var("QUEUE_URL").unwrap_or_else(|_| "http://localhost:8083".to_string()),
            data_engine_url: env::var("DATA_ENGINE_URL").unwrap_or_else(|_| "http://localhost:8082".to_string()),
            internal_service_token: env::var("INTERNAL_SERVICE_TOKEN").expect("INTERNAL_SERVICE_TOKEN required"),
            port: env::var("PORT")
                .or_else(|_| env::var("GATEWAY_PORT"))
                .unwrap_or_else(|_| "8081".to_string())
                .parse()
                .expect("PORT must be a number"),
        }
    }
}
