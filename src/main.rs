use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::io::Write;
use tiny::{Agent, Config, Decision, Event, OpenAiProvider, Tool};
use tokio::io::{AsyncBufReadExt, BufReader};

// --- ReadTool: shows how to implement a tool ---

struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str { "read" }
    fn description(&self) -> &str { "Read the contents of a file from disk." }
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

// --- main ---

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::load()?;
    let api_key = cfg.api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .context("set api_key in tiny.json or OPENAI_API_KEY in your environment")?;
    let model = cfg.model.unwrap_or_else(|| "gpt-4o-mini".to_string());
    let system = cfg.system.unwrap_or_else(|| "You are a helpful assistant.".to_string());

    let mut agent = Agent::new(OpenAiProvider::new(api_key, model), system)
        .with_permission(|name, _input| match name {
            "read" => Decision::Allow,
            other => Decision::Deny(format!("tool '{other}' is not permitted")),
        });
    agent.register_tool(ReadTool);

    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    loop {
        print!("> ");
        std::io::stdout().flush()?;

        let line = match lines.next_line().await? {
            None => break,
            Some(l) => l,
        };
        let line = line.trim().to_string();
        if line.is_empty() { continue; }

        agent.send(line, |event| {
            if let Event::AssistantText(text) = event {
                println!("{text}");
            }
        }).await?;
    }

    Ok(())
}
