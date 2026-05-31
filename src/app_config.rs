use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tiny::providers::{LlamaCppProvider, OpenAiProvider};
use tiny::{AgentConfig, ErasedTool};

#[derive(Deserialize, Default)]
pub struct Config {
    provider: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
}

#[derive(Clone, Copy)]
enum ProviderChoice {
    OpenAi,
    LlamaCpp,
}

impl ProviderChoice {
    fn from_config(cfg: &Config) -> Result<Self> {
        let Some(provider) = cfg.provider.as_deref() else {
            return Ok(if cfg.has_openai_key() {
                Self::OpenAi
            } else {
                Self::LlamaCpp
            });
        };

        match provider.trim().to_ascii_lowercase().as_str() {
            "openai" => Ok(Self::OpenAi),
            "llama.cpp" | "llama_cpp" | "llamacpp" => Ok(Self::LlamaCpp),
            other => bail!("unknown provider: {other}"),
        }
    }

    fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4o-mini",
            Self::LlamaCpp => "local",
        }
    }
}

impl Config {
    pub fn agent_config(
        &self,
        system: String,
        tools: Vec<Box<dyn ErasedTool>>,
    ) -> Result<(AgentConfig, String)> {
        let provider = ProviderChoice::from_config(self)?;
        let model = self.model_for(provider);
        let config = match provider {
            ProviderChoice::OpenAi => {
                let api_key = self
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                    .context("set api_key in tiny.json or OPENAI_API_KEY in your environment")?;
                AgentConfig::new(OpenAiProvider::new(api_key, &model), system).with_tools(tools)
            }
            ProviderChoice::LlamaCpp => {
                let base_url = self
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
                AgentConfig::new(LlamaCppProvider::new(base_url, &model), system).with_tools(tools)
            }
        };

        Ok((config, model))
    }

    fn has_openai_key(&self) -> bool {
        self.api_key.is_some() || std::env::var_os("OPENAI_API_KEY").is_some()
    }

    fn model_for(&self, provider: ProviderChoice) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| provider.default_model().to_string())
    }
}

pub fn load_config() -> Result<Config> {
    if let Ok(text) = std::fs::read_to_string("tiny.json") {
        return Ok(serde_json::from_str(&text)?);
    }

    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(".tiny").join("config.json");
        if let Ok(text) = std::fs::read_to_string(path) {
            return Ok(serde_json::from_str(&text)?);
        }
    }

    Ok(Config::default())
}
