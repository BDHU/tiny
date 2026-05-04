use crate::message::{ContentBlock, Message, Role};
use crate::provider::Provider;
use crate::tool::Tool;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct OpenAiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[&dyn Tool],
    ) -> Result<Message> {
        let mut wire_messages = vec![json!({"role": "system", "content": system})];
        for msg in messages {
            wire_messages.extend(message_to_wire(msg));
        }

        let mut body = json!({
            "model": self.model,
            "max_completion_tokens": 4096,
            "messages": wire_messages,
        });
        if !tools.is_empty() {
            body["tools"] = json!(tools.iter().map(|t| tool_to_wire(*t)).collect::<Vec<_>>());
        }

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("openai request failed")?;

        let status = response.status();
        let text = response.text().await.context("read response body")?;
        if !status.is_success() {
            return Err(anyhow!("openai {}: {}", status, text));
        }

        let value: Value = serde_json::from_str(&text).context("parse response json")?;
        wire_to_message(&value)
    }
}

// OpenAI tool results are separate top-level messages with role "tool",
// unlike Anthropic where they are blocks inside a user message.
fn message_to_wire(msg: &Message) -> Vec<Value> {
    match msg.role {
        Role::User => {
            let has_tool_results = msg
                .content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolResult { .. }));

            if has_tool_results {
                msg.content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } => Some(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content,
                        })),
                        _ => None,
                    })
                    .collect()
            } else {
                vec![json!({"role": "user", "content": msg.text_concat()})]
            }
        }
        Role::Assistant => {
            let text = msg.text_concat();
            let tool_calls: Vec<Value> = msg
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, name, input } => Some(json!({
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": input.to_string()},
                    })),
                    _ => None,
                })
                .collect();

            let mut wire = json!({"role": "assistant"});
            wire["content"] = if text.is_empty() { Value::Null } else { json!(text) };
            if !tool_calls.is_empty() {
                wire["tool_calls"] = json!(tool_calls);
            }
            vec![wire]
        }
    }
}

fn tool_to_wire(tool: &dyn Tool) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name(),
            "description": tool.description(),
            "parameters": tool.input_schema(),
        }
    })
}

fn wire_to_message(value: &Value) -> Result<Message> {
    let msg = &value["choices"][0]["message"];
    let mut content = Vec::new();

    if let Some(text) = msg["content"].as_str() {
        if !text.is_empty() {
            content.push(ContentBlock::Text(text.to_string()));
        }
    }

    if let Some(tool_calls) = msg["tool_calls"].as_array() {
        for tc in tool_calls {
            let id = tc["id"].as_str().unwrap_or("").to_string();
            let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
            let args = tc["function"]["arguments"].as_str().unwrap_or("{}");
            let input: Value = serde_json::from_str(args).context("parse tool arguments")?;
            content.push(ContentBlock::ToolUse { id, name, input });
        }
    }

    if content.is_empty() {
        return Err(anyhow!("empty response: {}", value));
    }

    Ok(Message {
        role: Role::Assistant,
        content,
    })
}
