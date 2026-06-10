use tokio::sync::{mpsc, oneshot};

use super::{Message, ToolCall};

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
