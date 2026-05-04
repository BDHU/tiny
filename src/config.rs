use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Default)]
pub struct Config {
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub system: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        // project-level takes precedence
        if let Ok(text) = std::fs::read_to_string("tiny.json") {
            return Ok(serde_json::from_str(&text)?);
        }
        if let Some(path) = user_config_path() {
            if let Ok(text) = std::fs::read_to_string(path) {
                return Ok(serde_json::from_str(&text)?);
            }
        }
        Ok(Self::default())
    }
}

fn user_config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".tiny").join("config.json"))
}
