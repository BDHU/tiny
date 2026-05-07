use crate::tui::{
    prompt::{Prompt, View},
    state::AppState,
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
    let session = state.session.as_ref();
    let model = session.map(|s| s.model.as_str()).unwrap_or("unknown");
    let message_count = session.map(|s| s.message_count).unwrap_or(0);

    let queued = if state.modal.is_none() {
        state.turn.as_ref().map(|t| t.queued).unwrap_or(0)
    } else {
        0
    };

    let view = View {
        input: &state.input,
        palette_index: state.palette_index,
        modal: state.modal.as_deref(),
        turn_started_at: state.turn.as_ref().map(|turn| turn.started_at),
        queued,
        model,
        directory_label: &state.directory_label,
        message_count,
        terminal_size: term_size,
    };
    prompt.render(out, &view)?;
    Ok(())
}
