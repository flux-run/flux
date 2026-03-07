use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub api_key: Option<String>,
    pub project_id: Option<String>,
}

impl Config {
    fn config_path() -> PathBuf {
        let mut path = dirs::home_dir().expect("Could not find home directory");
        path.push(".fluxbase");
        path.push("config.json");
        path
    }

    pub async fn load() -> Self {
        let path = Self::config_path();
        if !path.exists() {
            return Config::default();
        }

        let contents = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => return Config::default(),
        };

        serde_json::from_str(&contents).unwrap_or_default()
    }

    pub async fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        fs::write(path, contents).await?;
        Ok(())
    }
}
