use crate::compact;
use crate::tool::{boxed_tool, ErasedTool, Tool};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    User(String),
    Assistant {
        text: String,
        tool_calls: Vec<ToolCall>,
    },
    Tool(ToolResult),
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Box<dyn ErasedTool>],
    ) -> Result<Message>;
}

#[derive(Debug, Clone)]
pub enum Decision {
    Allow,
    Deny(String),
}

pub enum Event {
    Message(Message),
    PermissionRequest {
        call: ToolCall,
        reply: oneshot::Sender<Decision>,
    },
    TurnError(String),
    TurnDone,
}

pub type EventSender = mpsc::UnboundedSender<Event>;

pub struct AgentConfig {
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn ErasedTool>>,
    system: String,
    compact_threshold: usize,
}

impl AgentConfig {
    pub fn new(provider: impl Provider + 'static, system: impl Into<String>) -> Self {
        Self {
            provider: Box::new(provider),
            tools: Vec::new(),
            system: system.into(),
            compact_threshold: compact::DEFAULT_THRESHOLD,
        }
    }

    pub fn with_tool(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.push(boxed_tool(tool));
        self
    }

    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Box<dyn ErasedTool>>) -> Self {
        self.tools.extend(tools);
        self
    }

    pub fn with_compact_threshold(mut self, chars: usize) -> Self {
        self.compact_threshold = chars;
        self
    }
}

pub struct Agent {
    config: Arc<AgentConfig>,
    pub history: Vec<Message>,
}

impl Agent {
    pub fn new(config: Arc<AgentConfig>, history: Vec<Message>) -> Self {
        Self { config, history }
    }

    pub async fn compact(&mut self) -> Result<bool> {
        compact::compact_now(&mut self.history, &*self.config.provider).await
    }

    pub async fn send(
        &mut self,
        user_input: impl Into<String>,
        events: &EventSender,
    ) -> Result<()> {
        let result = self.run_turn(user_input.into(), events).await;
        if let Err(error) = &result {
            let _ = events.send(Event::TurnError(error.to_string()));
        } else {
            let _ = compact::compact_if_needed(
                &mut self.history,
                &*self.config.provider,
                self.config.compact_threshold,
            )
            .await;
        }
        let _ = events.send(Event::TurnDone);
        result
    }

    async fn run_turn(&mut self, user_input: String, events: &EventSender) -> Result<()> {
        self.record(Message::User(user_input), events);

        loop {
            let assistant = self
                .config
                .provider
                .complete(&self.config.system, &self.history, &self.config.tools)
                .await?;

            let Message::Assistant { tool_calls, .. } = &assistant else {
                return Err(anyhow!("provider returned a non-assistant message"));
            };
            let calls = tool_calls.clone();
            self.record(assistant, events);
            if calls.is_empty() {
                return Ok(());
            }
            for call in calls {
                let result = self.call_tool(call, events).await;
                self.record(Message::Tool(result), events);
            }
        }
    }

    fn record(&mut self, message: Message, events: &EventSender) {
        let _ = events.send(Event::Message(message.clone()));
        self.history.push(message);
    }

    async fn call_tool(&self, call: ToolCall, events: &EventSender) -> ToolResult {
        let (content, is_error) = match self.ask_permission(&call, events).await {
            Decision::Allow => match self.config.tools.iter().find(|t| t.name() == call.name) {
                Some(tool) => match tool.call(call.input).await {
                    Ok(out) => (out, false),
                    Err(e) => (e.to_string(), true),
                },
                None => (format!("unknown tool: {}", call.name), true),
            },
            Decision::Deny(reason) => (reason, true),
        };

        ToolResult {
            id: call.id,
            content,
            is_error,
        }
    }

    async fn ask_permission(&self, call: &ToolCall, events: &EventSender) -> Decision {
        let (reply, decision) = oneshot::channel();
        let request = Event::PermissionRequest {
            call: call.clone(),
            reply,
        };
        if events.send(request).is_err() {
            return Decision::Deny("permission channel closed".into());
        }
        decision
            .await
            .unwrap_or_else(|_| Decision::Deny("permission cancelled".into()))
    }
}
