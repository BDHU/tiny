use tokio::sync::oneshot;

use super::{AgentConfig, Decision, Event, EventSender, ToolCall, ToolResult};

pub(crate) async fn execute_tool(
    config: &AgentConfig,
    events: &EventSender,
    call: ToolCall,
) -> ToolResult {
    let (content, is_error) = match ask_permission(config, events, &call).await {
        Decision::Allow => call_allowed_tool(config, &call).await,
        Decision::Deny(reason) => (reason, true),
    };

    ToolResult {
        id: call.id,
        content,
        is_error,
    }
}

async fn call_allowed_tool(config: &AgentConfig, call: &ToolCall) -> (String, bool) {
    let Some(tool) = config.tools.iter().find(|tool| tool.name() == call.name) else {
        return (format!("unknown tool: {}", call.name), true);
    };

    match tool.call(call.input.clone()).await {
        Ok(output) => (output, false),
        Err(error) => (error.to_string(), true),
    }
}

async fn ask_permission(config: &AgentConfig, events: &EventSender, call: &ToolCall) -> Decision {
    if config.auto_allow_tools {
        return Decision::Allow;
    }

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
