use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use tiny::{Agent, Decision, OpenAiProvider, Tool};

mod tui;

#[derive(Deserialize, Default)]
struct Config {
    api_key: Option<String>,
    model: Option<String>,
    system: Option<String>,
}

fn load_config() -> Result<Config> {
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

struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Read the contents of a file from disk."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file" }
            },
            "required": ["path"]
        })
    }
    async fn call(&self, input: Value) -> Result<String> {
        let path = input["path"].as_str().context("missing path")?;
        Ok(std::fs::read_to_string(path)?)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = load_config()?;
    let api_key = cfg
        .api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .context("set api_key in tiny.json or OPENAI_API_KEY in your environment")?;
    let model = cfg.model.unwrap_or_else(|| "gpt-4o-mini".to_string());
    let system = cfg
        .system
        .unwrap_or_else(|| "You are a helpful assistant.".to_string());

    let mut agent = Agent::new(OpenAiProvider::new(api_key, model.clone()), system)
        .with_permission(|name, _input| match name {
            "read" => Decision::Allow,
            other => Decision::Deny(format!("tool '{other}' is not permitted")),
        });
    agent.register_tool(ReadTool);

    tui::run(agent, model).await
}
