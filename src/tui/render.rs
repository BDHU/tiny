use crate::tui::{
    prompt::{Prompt, View},
    state::{AppState, Modal},
};
use anyhow::Result;
use crossterm::terminal;
use std::io::Write;

pub(crate) fn render_screen<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &AppState,
) -> Result<()> {
    let term_size = terminal::size().unwrap_or((80, 24));
    let model = state
        .session
        .as_ref()
        .map(|s| s.model.as_str())
        .unwrap_or("unknown");
    let message_count = state.session.as_ref().map(|s| s.message_count).unwrap_or(0);
    let active_session_id = state.session.as_ref().map(|s| s.id.as_str());

    let pending_call = match state.modal.as_ref() {
        Some(Modal::PermissionPrompt(_, call)) => Some(call),
        _ => None,
    };
    let queued = if state.modal.is_none() {
        state.turn.as_ref().map(|t| t.queued).unwrap_or(0)
    } else {
        0
    };

    let view = View {
        input: &state.input,
        palette_index: state.palette_index,
        picker: match state.modal.as_ref() {
            Some(Modal::SessionPicker(picker)) => Some(picker),
            _ => None,
        },
        pending_call,
        turn_started_at: state.turn.as_ref().map(|turn| turn.started_at),
        queued,
        model,
        directory_label: &state.directory_label,
        message_count,
        active_session_id,
        terminal_size: term_size,
    };
    prompt.render(out, &view)?;
    Ok(())
}
