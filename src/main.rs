use anyhow::Result;
use std::sync::Arc;
use tiny::{AgentConfig, ErasedTool};

mod app_config;
mod backend;
mod subagent;
mod tools;
mod toolset;
mod tui;
mod web;
mod web_fetch;
mod web_search;

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = app_config::load_config()?;
    let (provider, model) = cfg.provider()?;
    let tools = tools::default_tools(provider.clone());
    let system = default_system_prompt(&tools);
    let config = AgentConfig::new_with_provider(provider, system).with_tools(tools);

    tui::run(Arc::new(config), model).await
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
