use crate::message::{ContentBlock, Message, Role};
use crate::tool::Tool;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[&dyn Tool],
    ) -> Result<Message>;
}

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[&dyn Tool],
    ) -> Result<Message> {
        let body = json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": system,
            "messages": messages.iter().map(message_to_wire).collect::<Vec<_>>(),
            "tools": tools.iter().map(|t| tool_to_wire(*t)).collect::<Vec<_>>(),
        });

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("anthropic request failed")?;

        let status = response.status();
        let text = response.text().await.context("read response body")?;
        if !status.is_success() {
            return Err(anyhow!("anthropic {}: {}", status, text));
        }
        let value: Value = serde_json::from_str(&text).context("parse response json")?;
        wire_to_message(&value)
    }
}

fn message_to_wire(msg: &Message) -> Value {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<Value> = msg.content.iter().map(block_to_wire).collect();
    json!({ "role": role, "content": content })
}

fn block_to_wire(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text(t) => json!({ "type": "text", "text": t }),
        ContentBlock::ToolUse { id, name, input } => {
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": content,
            "is_error": is_error,
        }),
    }
}

fn tool_to_wire(tool: &dyn Tool) -> Value {
    json!({
        "name": tool.name(),
        "description": tool.description(),
        "input_schema": tool.input_schema(),
    })
}

fn wire_to_message(value: &Value) -> Result<Message> {
    let blocks = value
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("response missing content array: {}", value))?;
    let mut content = Vec::with_capacity(blocks.len());
    for b in blocks {
        let kind = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match kind {
            "text" => {
                let text = b.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                content.push(ContentBlock::Text(text));
            }
            "tool_use" => {
                let id = b.get("id").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let name = b.get("name").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let input = b.get("input").cloned().unwrap_or(Value::Null);
                content.push(ContentBlock::ToolUse { id, name, input });
            }
            other => return Err(anyhow!("unknown content block type: {}", other)),
        }
    }
    Ok(Message {
        role: Role::Assistant,
        content,
    })
}
