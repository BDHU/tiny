use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use tiny::{Agent, AgentConfig, Event, Message, Provider};
use tokio::sync::mpsc;

use super::registry::SubagentSpec;
use super::toolsets;

pub(crate) async fn run(
    provider: Arc<dyn Provider>,
    spec: &'static SubagentSpec,
    task: String,
) -> Result<String> {
    if task.trim().is_empty() {
        bail!("delegate task must not be empty");
    }

    let config = AgentConfig::new_with_provider(provider, spec.system)
        .with_tools(toolsets::for_subagent(spec))
        .with_auto_allow_tools();
    let mut agent = Agent::new(Arc::new(config), Vec::new());
    let (events, mut event_rx) = mpsc::unbounded_channel::<Event>();
    let drain_events = tokio::spawn(async move { while event_rx.recv().await.is_some() {} });

    agent.send(format_task(spec, &task), &events).await?;
    drop(events);
    let _ = drain_events.await;

    final_text(&agent.history)
        .map(|text| format!("{} report:\n{}", spec.name, text.trim()))
        .ok_or_else(|| anyhow!("subagent '{}' did not return a text response", spec.name))
}

fn format_task(spec: &SubagentSpec, task: &str) -> String {
    format!(
        "Subagent: {}\nDescription: {}\n\nTask:\n{}",
        spec.name,
        spec.description,
        task.trim()
    )
}

fn final_text(history: &[Message]) -> Option<&str> {
    history.iter().rev().find_map(|message| match message {
        Message::Assistant { text, .. } if !text.trim().is_empty() => Some(text.as_str()),
        Message::User(_) | Message::Assistant { .. } | Message::Tool(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny::ToolCall;

    #[test]
    fn final_text_uses_last_assistant_text() {
        let history = vec![
            Message::Assistant {
                text: "first".into(),
                tool_calls: Vec::new(),
            },
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "1".into(),
                    name: "read".into(),
                    input: serde_json::json!({ "path": "src/main.rs" }),
                }],
            },
            Message::Assistant {
                text: "last".into(),
                tool_calls: Vec::new(),
            },
        ];

        assert_eq!(final_text(&history), Some("last"));
    }
}
