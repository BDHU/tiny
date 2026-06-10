use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;

use crate::tool::{ErasedTool, Tool};

use super::{Agent, AgentConfig, Message, Provider, ToolCall, ToolResult};

struct ScriptedProvider {
    replies: Mutex<VecDeque<Message>>,
}

impl ScriptedProvider {
    fn new(replies: impl IntoIterator<Item = Message>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
        }
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    async fn complete(
        &self,
        _system: &str,
        _messages: &[Message],
        _tools: &[Box<dyn ErasedTool>],
    ) -> Result<Message> {
        self.replies
            .lock()
            .expect("scripted provider mutex poisoned")
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("no scripted replies left"))
    }
}

struct EchoTool;

#[derive(Deserialize, JsonSchema)]
struct EchoArgs {
    text: String,
}

#[async_trait]
impl Tool for EchoTool {
    type Args = EchoArgs;

    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "echo text"
    }

    async fn call(&self, args: Self::Args) -> Result<String> {
        Ok(args.text)
    }
}

#[tokio::test]
async fn send_runs_tool_calls_until_final_assistant() {
    let provider = ScriptedProvider::new([
        Message::Assistant {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".into(),
                name: "echo".into(),
                input: json!({ "text": "hi" }),
            }],
        },
        Message::Assistant {
            text: "done".into(),
            tool_calls: Vec::new(),
        },
    ]);
    let config = AgentConfig::new(provider, "system")
        .with_tool(EchoTool)
        .with_auto_allow_tools();
    let mut agent = Agent::new(Arc::new(config), Vec::new());
    let (events, _rx) = mpsc::unbounded_channel();

    agent.send("hello", &events).await.unwrap();

    assert!(matches!(agent.history[0], Message::User(ref text) if text == "hello"));
    assert!(matches!(
        agent.history[2],
        Message::Tool(ToolResult {
            ref content,
            is_error: false,
            ..
        }) if content == "hi"
    ));
    assert!(matches!(
        agent.history[3],
        Message::Assistant { ref text, .. } if text == "done"
    ));
}
