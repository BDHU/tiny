use crate::backend::PermissionId;
use crate::tui::{input::InputBuffer, picker::SessionPicker};
use std::time::Instant;
use tiny::{Message, SessionMeta, ToolCall};

pub(crate) enum Modal {
    SessionPicker(SessionPicker),
    PermissionPrompt(PermissionId, ToolCall),
}

pub(crate) struct TurnState {
    pub(crate) started_at: Instant,
    pub(crate) queued: usize,
}

pub(crate) struct SessionState {
    pub(crate) id: String,
    pub(crate) model: String,
    pub(crate) message_count: usize,
}

pub(crate) struct AppState {
    pub(crate) input: InputBuffer,
    pub(crate) palette_index: usize,
    pub(crate) session: Option<SessionState>,
    pub(crate) modal: Option<Modal>,
    pub(crate) turn: Option<TurnState>,
    pub(crate) directory_label: String,
}

impl AppState {
    pub(crate) fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let directory_label = cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| cwd.display().to_string());
        Self {
            input: InputBuffer::default(),
            palette_index: 0,
            session: None,
            modal: None,
            turn: None,
            directory_label,
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.turn.is_some()
    }

    pub(crate) fn has_session(&self) -> bool {
        self.session.is_some()
    }

    pub(crate) fn set_session(&mut self, meta: SessionMeta, history: &[Message]) {
        self.session = Some(SessionState {
            id: meta.id.0,
            model: meta.model,
            message_count: count_chat_messages(history),
        });
    }

    pub(crate) fn record_chat_message(&mut self) {
        if let Some(session) = &mut self.session {
            session.message_count += 1;
        }
    }

    /// Local input was submitted: start a turn or queue behind the current one.
    pub(crate) fn turn_submitted(&mut self) {
        match &mut self.turn {
            Some(turn) => turn.queued += 1,
            None => {
                self.turn = Some(TurnState {
                    started_at: Instant::now(),
                    queued: 0,
                });
            }
        }
    }

    /// Backend signalled it picked up a turn: dequeue one from our counter, or
    /// start the timer if this is the first we've seen (e.g. resuming).
    pub(crate) fn turn_started_by_backend(&mut self) {
        match &mut self.turn {
            Some(turn) if turn.queued > 0 => turn.queued -= 1,
            Some(_) => {}
            None => {
                self.turn = Some(TurnState {
                    started_at: Instant::now(),
                    queued: 0,
                });
            }
        }
    }
}

fn count_chat_messages(history: &[Message]) -> usize {
    history
        .iter()
        .filter(|m| matches!(m, Message::User(_) | Message::Assistant { .. }))
        .count()
}
