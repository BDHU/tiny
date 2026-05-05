use crate::message::{Message, ToolCall};
use crate::provider::Provider;
use crate::tool::ErasedTool;
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
        tools: &[Box<dyn ErasedTool>],
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
            body["tools"] = json!(tools
                .iter()
                .map(|tool| tool_to_wire(tool.as_ref()))
                .collect::<Vec<_>>());
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

fn message_to_wire(msg: &Message) -> Vec<Value> {
    match msg {
        Message::User(text) => vec![json!({"role": "user", "content": text})],
        Message::Assistant { text, tool_calls } => {
            let mut wire = json!({"role": "assistant"});
            wire["content"] = if text.is_empty() {
                Value::Null
            } else {
                json!(text)
            };

            let tool_calls: Vec<Value> = tool_calls
                .iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": call.input.to_string(),
                        },
                    })
                })
                .collect();
            if !tool_calls.is_empty() {
                wire["tool_calls"] = json!(tool_calls);
            }

            vec![wire]
        }
        Message::Tool(result) => vec![json!({
            "role": "tool",
            "tool_call_id": result.id,
            "content": result.content,
        })],
    }
}

fn tool_to_wire(tool: &dyn ErasedTool) -> Value {
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
    let text = msg["content"].as_str().unwrap_or("").to_string();
    let mut tool_calls = Vec::new();
    if let Some(wire_tool_calls) = msg["tool_calls"].as_array() {
        for tc in wire_tool_calls {
            let id = tc["id"].as_str().unwrap_or("").to_string();
            let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
            let args = tc["function"]["arguments"].as_str().unwrap_or("{}");
            let input: Value = serde_json::from_str(args).context("parse tool arguments")?;
            tool_calls.push(ToolCall { id, name, input });
        }
    }

    if text.is_empty() && tool_calls.is_empty() {
        return Err(anyhow!("empty response: {}", value));
    }

    Ok(Message::Assistant { text, tool_calls })
}
