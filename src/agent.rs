use crate::message::{ContentBlock, Message, Role};
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
        self.history.push(Message::user_text(user_input));
        loop {
            let tool_refs: Vec<&dyn Tool> = self.tools.iter().map(|t| t.as_ref()).collect();
            let assistant = self
                .provider
                .complete(&self.system, &self.history, &tool_refs)
                .await?;
            self.history.push(assistant.clone());

            let text = assistant.text_concat();
            if !text.is_empty() {
                on_event(&Event::AssistantText(text));
            }

            let tool_uses: Vec<_> = assistant
                .tool_uses()
                .map(|(id, name, input)| (id.to_string(), name.to_string(), input.clone()))
                .collect();

            if tool_uses.is_empty() {
                return Ok(());
            }

            let mut results = Vec::new();
            for (id, name, input) in tool_uses {
                on_event(&Event::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
                let (content, is_error) = match (self.permission)(&name, &input) {
                    Decision::Allow => match self.dispatch(&name, input).await {
                        Ok(out) => (out, false),
                        Err(e) => (e.to_string(), true),
                    },
                    Decision::Deny(reason) => (reason, true),
                };
                on_event(&Event::ToolResult {
                    id: id.clone(),
                    content: content.clone(),
                    is_error,
                });
                results.push(ContentBlock::ToolResult {
                    tool_use_id: id,
                    content,
                    is_error,
                });
            }
            self.history.push(Message {
                role: Role::User,
                content: results,
            });
        }
    }

    async fn dispatch(&self, name: &str, input: Value) -> Result<String> {
        match self.tools.iter().find(|t| t.name() == name) {
            Some(tool) => tool.call(input).await,
            None => Err(anyhow::anyhow!("unknown tool: {}", name)),
        }
    }
}
