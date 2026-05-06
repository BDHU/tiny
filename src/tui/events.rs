use crate::{
    backend::BackendEvent,
    tui::{
        reader::ReaderEvent,
        state::UiEvent,
        transcript::{entries_from_message, Entry},
    },
};
use crossterm::event::{Event as CtEvent, KeyEventKind, MouseEventKind};
use tiny::Message;

pub(crate) fn from_backend(event: BackendEvent) -> Vec<UiEvent> {
    match event {
        // The agent records the user message as the first event of every turn.
        // The TUI already showed it locally on Enter, so suppress the duplicate.
        BackendEvent::Message(Message::User(_)) => Vec::new(),
        BackendEvent::Message(message) => entries_from_message(message)
            .into_iter()
            .map(UiEvent::Entry)
            .collect(),
        BackendEvent::PermissionRequest { id, call } => {
            vec![UiEvent::PermissionRequest { id, call }]
        }
        BackendEvent::TurnStarted => vec![UiEvent::TurnStarted],
        BackendEvent::TurnError(error) => vec![UiEvent::TurnError(error)],
        BackendEvent::TurnDone => vec![UiEvent::TurnDone],
        BackendEvent::SessionChanged { meta, history } => {
            vec![UiEvent::SessionChanged { meta, history }]
        }
        BackendEvent::SessionsListed(result) => vec![UiEvent::SessionsListed(result)],
        BackendEvent::SessionError(error) => vec![UiEvent::Entry(Entry::Error(error))],
    }
}

pub(crate) fn from_reader(event: ReaderEvent) -> Vec<UiEvent> {
    match event {
        ReaderEvent::Terminal(CtEvent::Key(key)) if key.kind == KeyEventKind::Press => {
            vec![UiEvent::Key(key)]
        }
        ReaderEvent::Terminal(CtEvent::Paste(text)) => vec![UiEvent::Paste(text)],
        ReaderEvent::Terminal(CtEvent::Mouse(mouse)) => match mouse.kind {
            MouseEventKind::ScrollUp => vec![UiEvent::Scroll(-1)],
            MouseEventKind::ScrollDown => vec![UiEvent::Scroll(1)],
            _ => Vec::new(),
        },
        ReaderEvent::Terminal(CtEvent::Resize(_, _)) => Vec::new(),
        ReaderEvent::Terminal(_) => Vec::new(),
        ReaderEvent::Error(error) => vec![UiEvent::Entry(Entry::Error(error))],
    }
}
