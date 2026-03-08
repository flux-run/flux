use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub api_url: String,
    pub token: Option<String>,
    pub tenant_id: Option<String>,
    pub tenant_slug: Option<String>,
    pub project_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url: "https://api.fluxbase.co".to_string(),
            token: None,
            tenant_id: None,
            tenant_slug: None,
            project_id: None,
        }
    }
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
        
        let mut config = if path.exists() {
            let contents = fs::read_to_string(&path).await.unwrap_or_default();
            serde_json::from_str(&contents).unwrap_or_else(|_| Config::default())
        } else {
            Config::default()
        };

        if let Ok(url) = std::env::var("FLUXBASE_API_URL") {
            config.api_url = url;
        }

        config
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
