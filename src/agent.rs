use crate::message::{Message, ToolCall, ToolResult};
use crate::permission::Decision;
use crate::provider::Provider;
use crate::tool::Tool;
use anyhow::Result;
use serde_json::Value;

pub enum Event {
    AssistantText(String),
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        id: String,
        content: String,
        is_error: bool,
    },
}

type PermissionFn = Box<dyn Fn(&str, &Value) -> Decision + Send + Sync>;

pub struct Agent {
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    system: String,
    permission: PermissionFn,
    pub history: Vec<Message>,
}

impl Agent {
    pub fn new(provider: impl Provider + 'static, system: impl Into<String>) -> Self {
        Self {
            provider: Box::new(provider),
            tools: Vec::new(),
            system: system.into(),
            permission: Box::new(|_, _| Decision::Allow),
            history: Vec::new(),
        }
    }

    pub fn register_tool(&mut self, tool: impl Tool + 'static) -> &mut Self {
        self.tools.push(Box::new(tool));
        self
    }

    pub fn with_permission(
        mut self,
        f: impl Fn(&str, &Value) -> Decision + Send + Sync + 'static,
    ) -> Self {
        self.permission = Box::new(f);
        self
    }

    pub async fn send(
        &mut self,
        user_input: impl Into<String>,
        mut on_event: impl FnMut(&Event),
    ) -> Result<()> {
        self.history.push(Message::User(user_input.into()));
        loop {
            let tool_refs: Vec<&dyn Tool> = self.tools.iter().map(|t| t.as_ref()).collect();
            let assistant = self
                .provider
                .complete(&self.system, &self.history, &tool_refs)
                .await?;
            self.history.push(assistant.clone());

            let Message::Assistant { text, tool_calls } = assistant else {
                return Err(anyhow::anyhow!("provider returned a non-assistant message"));
            };

            if !text.is_empty() {
                on_event(&Event::AssistantText(text));
            }

            if tool_calls.is_empty() {
                return Ok(());
            }

            for tool_call in tool_calls {
                on_event(&Event::ToolCall {
                    id: tool_call.id.clone(),
                    name: tool_call.name.clone(),
                    input: tool_call.input.clone(),
                });
                let result = self.call_tool(tool_call).await;
                on_event(&Event::ToolResult {
                    id: result.id.clone(),
                    content: result.content.clone(),
                    is_error: result.is_error,
                });
                self.history.push(Message::Tool(result));
            }
        }
    }

    async fn call_tool(&self, tool_call: ToolCall) -> ToolResult {
        let (content, is_error) = match (self.permission)(&tool_call.name, &tool_call.input) {
            Decision::Allow => match self.dispatch(&tool_call.name, tool_call.input).await {
                Ok(out) => (out, false),
                Err(e) => (e.to_string(), true),
            },
            Decision::Deny(reason) => (reason, true),
        };

        ToolResult {
            id: tool_call.id,
            content,
            is_error,
        }
    }

    async fn dispatch(&self, name: &str, input: Value) -> Result<String> {
        match self.tools.iter().find(|t| t.name() == name) {
            Some(tool) => tool.call(input).await,
            None => Err(anyhow::anyhow!("unknown tool: {}", name)),
        }
    }
}
