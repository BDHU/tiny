use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tiny::{AgentConfig, ErasedTool, OpenAiProvider};

mod backend;
mod tools;
mod toolset;
mod tui;
mod web;
mod web_fetch;
mod web_search;

#[derive(Deserialize, Default)]
struct Config {
    api_key: Option<String>,
    model: Option<String>,
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

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = load_config()?;
    let api_key = cfg
        .api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .context("set api_key in tiny.json or OPENAI_API_KEY in your environment")?;
    let model = cfg.model.unwrap_or_else(|| "gpt-4o-mini".to_string());
    let tools = tools::default_tools();
    let system = default_system_prompt(&tools);

    let config = Arc::new(
        AgentConfig::new(OpenAiProvider::new(api_key, model.clone()), system).with_tools(tools),
    );

    tui::run(config, model).await
}

fn default_system_prompt(tools: &[Box<dyn ErasedTool>]) -> String {
    let tool_list: String = tools
        .iter()
        .map(|t| format!("- {}: {}\n", t.name(), t.description()))
        .collect();
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    let date = chrono::Local::now().format("%Y-%m-%d");
    format!(
        "You are a coding assistant running inside tiny, a small terminal agent harness.\n\n\
Current date: {date}\n\
Current working directory: {cwd}\n\n\
Available tools:\n{tool_list}\n\
Guidelines:\n\
- Be concise and direct.\n\
- Read relevant files before changing code.\n\
- Prefer structured tools for file work: read, edit, write, list, glob, and grep.\n\
- Use bash when shell commands, tests, builds, formatting, or git inspection are needed.\n\
- Show file paths clearly when discussing changes.\n\
- After code changes, run the smallest useful verification command and report the result.\n"
    )
}
