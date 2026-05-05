use crate::{
    backend::BackendEvent,
    tui::{reader::ReaderEvent, state::UiEvent, transcript::Entry},
};
use crossterm::event::{Event as CtEvent, KeyEventKind, MouseEventKind};
use tiny::Message;

pub(crate) fn from_backend(event: BackendEvent) -> Vec<UiEvent> {
    match event {
        // The agent records the user message as the first event of every turn.
        // The TUI already showed it locally on Enter, so suppress the duplicate.
        BackendEvent::Message(Message::User(_)) => Vec::new(),
        BackendEvent::Message(Message::Assistant { text, tool_calls }) => {
            let mut out = Vec::new();
            if !text.is_empty() {
                out.push(UiEvent::Entry(Entry::Assistant(text)));
            }
            out.extend(tool_calls.into_iter().map(|call| {
                UiEvent::Entry(Entry::ToolCall {
                    name: call.name,
                    args: call.input,
                })
            }));
            out
        }
        BackendEvent::Message(Message::Tool(result)) => vec![UiEvent::Entry(Entry::ToolResult {
            content: result.content,
            is_error: result.is_error,
        })],
        BackendEvent::PermissionRequest { id, call } => {
            vec![UiEvent::PermissionRequest { id, call }]
        }
        BackendEvent::TurnStarted => vec![UiEvent::TurnStarted],
        BackendEvent::TurnError(error) => vec![UiEvent::TurnError(error)],
        BackendEvent::TurnDone => vec![UiEvent::TurnDone],
    }
}

pub(crate) fn from_reader(event: ReaderEvent) -> Vec<UiEvent> {
    match event {
        ReaderEvent::Terminal(CtEvent::Key(key)) if key.kind == KeyEventKind::Press => {
            vec![UiEvent::Key(key)]
        }
        ReaderEvent::Terminal(CtEvent::Paste(text)) => vec![UiEvent::Paste(text)],
        ReaderEvent::Terminal(CtEvent::Mouse(mouse)) => match mouse.kind {
            MouseEventKind::ScrollUp => vec![UiEvent::Scroll(-3)],
            MouseEventKind::ScrollDown => vec![UiEvent::Scroll(3)],
            _ => Vec::new(),
        },
        ReaderEvent::Terminal(CtEvent::Resize(_, _)) => Vec::new(),
        ReaderEvent::Terminal(_) => Vec::new(),
        ReaderEvent::Error(error) => vec![UiEvent::Entry(Entry::Error(error))],
    }
}
