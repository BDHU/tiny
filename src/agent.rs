use crate::message::{Message, ToolCall, ToolResult};
use crate::provider::Provider;
use crate::tool::{boxed_tool, ErasedTool, Tool};
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

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
}

pub struct Agent {
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn ErasedTool>>,
    system: String,
    pub history: Vec<Message>,
}

impl Agent {
    pub fn new(provider: impl Provider + 'static, system: impl Into<String>) -> Self {
        Self {
            provider: Box::new(provider),
            tools: Vec::new(),
            system: system.into(),
            history: Vec::new(),
        }
    }

    pub fn register_tool(&mut self, tool: impl Tool + 'static) -> &mut Self {
        self.tools.push(boxed_tool(tool));
        self
    }

    pub fn register_tools(
        &mut self,
        tools: impl IntoIterator<Item = Box<dyn ErasedTool>>,
    ) -> &mut Self {
        self.tools.extend(tools);
        self
    }

    pub async fn send(
        &mut self,
        user_input: impl Into<String>,
        events: &mpsc::UnboundedSender<Event>,
    ) -> Result<()> {
        self.record(Message::User(user_input.into()), events);

        loop {
            let assistant = self
                .provider
                .complete(&self.system, &self.history, &self.tools)
                .await?;

            let Message::Assistant { tool_calls, .. } = &assistant else {
                return Err(anyhow::anyhow!("provider returned a non-assistant message"));
            };
            let tool_calls = tool_calls.clone();

            self.record(assistant, events);

            if tool_calls.is_empty() {
                return Ok(());
            }

            for tool_call in tool_calls {
                let result = self.call_tool(tool_call, events).await;
                self.record(Message::Tool(result), events);
            }
        }
    }

    fn record(&mut self, message: Message, events: &mpsc::UnboundedSender<Event>) {
        self.history.push(message);
        let message = self
            .history
            .last()
            .expect("message was just recorded")
            .clone();
        let _ = events.send(Event::Message(message));
    }

    async fn call_tool(
        &self,
        tool_call: ToolCall,
        events: &mpsc::UnboundedSender<Event>,
    ) -> ToolResult {
        let decision = self.ask_permission(&tool_call, events).await;
        let (content, is_error) = match decision {
            Decision::Allow => match self.tools.iter().find(|tool| tool.name() == tool_call.name) {
                Some(tool) => match tool.call(tool_call.input).await {
                    Ok(out) => (out, false),
                    Err(e) => (e.to_string(), true),
                },
                None => (format!("unknown tool: {}", tool_call.name), true),
            },
            Decision::Deny(reason) => (reason, true),
        };

        ToolResult {
            id: tool_call.id,
            content,
            is_error,
        }
    }

    async fn ask_permission(
        &self,
        tool_call: &ToolCall,
        events: &mpsc::UnboundedSender<Event>,
    ) -> Decision {
        let (reply, decision) = oneshot::channel();
        if events
            .send(Event::PermissionRequest {
                call: tool_call.clone(),
                reply,
            })
            .is_err()
        {
            return Decision::Deny("permission channel closed".into());
        }

        decision
            .await
            .unwrap_or_else(|_| Decision::Deny("permission cancelled".into()))
    }
}
