use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub telegram: TelegramConfig,
    pub gemini: GeminiConfig,
    pub storage: StorageConfig,
    pub web: WebConfig,
}

#[derive(Debug, Deserialize)]
pub struct TelegramConfig {
    pub channels: Vec<String>,
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub _session_file: Option<String>,
    // Loaded from env
    #[serde(skip)]
    pub api_id: i32,
    #[serde(skip)]
    pub api_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct GeminiConfig {
    pub model: String,
    pub max_concurrent: usize,
    pub base_url: String,
    // Loaded from env
    #[serde(skip)]
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct StorageConfig {
    pub data_dir: PathBuf,
    pub format: String,
}

#[derive(Debug, Deserialize)]
pub struct WebConfig {
    pub host: String,
    pub port: u16,
    pub recent_buffer_size: usize,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();

        let config_text =
            std::fs::read_to_string("config.toml").context("Failed to read config.toml")?;
        let mut config: AppConfig =
            toml::from_str(&config_text).context("Failed to parse config.toml")?;

        config.telegram.api_id = std::env::var("TG_API_ID")
            .context("TG_API_ID not set")?
            .parse()
            .context("TG_API_ID must be an integer")?;
        config.telegram.api_hash =
            std::env::var("TG_API_HASH").context("TG_API_HASH not set")?;
        config.gemini.api_key =
            std::env::var("GEMINI_API_KEY").context("GEMINI_API_KEY not set")?;

        Ok(config)
    }
}
