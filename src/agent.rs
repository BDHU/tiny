mod config;
mod event;
mod executor;
mod message;
mod provider;

pub use config::AgentConfig;
pub use event::{Decision, Event, EventSender};
pub use message::{Message, ToolCall, ToolResult};
pub use provider::Provider;

use anyhow::{anyhow, Result};
use std::sync::Arc;

use crate::compact;

use self::executor::execute_tool;

pub struct Agent {
    config: Arc<AgentConfig>,
    pub history: Vec<Message>,
}

impl Agent {
    pub fn new(config: Arc<AgentConfig>, history: Vec<Message>) -> Self {
        Self { config, history }
    }

    pub async fn compact(&mut self) -> Result<bool> {
        compact::compact_now(&mut self.history, self.config.provider.as_ref()).await
    }

    pub async fn send(
        &mut self,
        user_input: impl Into<String>,
        events: &EventSender,
    ) -> Result<()> {
        let result = self.run_turn(user_input.into(), events).await;
        self.finish_turn(&result, events).await;
        result
    }

    async fn run_turn(&mut self, user_input: String, events: &EventSender) -> Result<()> {
        self.record(Message::User(user_input), events);

        loop {
            let assistant = self.next_assistant_message().await?;
            let calls = assistant.tool_calls().to_vec();
            self.record(assistant, events);

            if calls.is_empty() {
                return Ok(());
            }

            self.run_tool_calls(calls, events).await;
        }
    }

    async fn next_assistant_message(&self) -> Result<Message> {
        let message = self
            .config
            .provider
            .complete(&self.config.system, &self.history, &self.config.tools)
            .await?;

        if !message.is_assistant() {
            return Err(anyhow!("provider returned a non-assistant message"));
        }

        Ok(message)
    }

    async fn run_tool_calls(&mut self, calls: Vec<ToolCall>, events: &EventSender) {
        for call in calls {
            let result = execute_tool(&self.config, events, call).await;
            self.record(Message::Tool(result), events);
        }
    }

    async fn finish_turn(&mut self, result: &Result<()>, events: &EventSender) {
        if let Err(error) = result {
            let _ = events.send(Event::TurnError(error.to_string()));
        } else {
            let _ = compact::compact_if_needed(
                &mut self.history,
                self.config.provider.as_ref(),
                self.config.compact_threshold,
            )
            .await;
        }

        let _ = events.send(Event::TurnDone);
    }

    fn record(&mut self, message: Message, events: &EventSender) {
        let _ = events.send(Event::Message(message.clone()));
        self.history.push(message);
    }
}

#[cfg(test)]
mod tests;
