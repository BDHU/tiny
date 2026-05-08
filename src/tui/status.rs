use crate::tui::{frame::fit_line, state::AppState, theme};
use crossterm::{
    queue,
    style::{Print, ResetColor, SetForegroundColor},
};
use std::io::{self, Write};

pub(crate) fn write_status<W: Write>(
    out: &mut W,
    state: &AppState,
    term_cols: u16,
) -> io::Result<()> {
    let text = status_text(state);
    queue!(
        out,
        SetForegroundColor(theme::DIM),
        Print(fit_line(&text, term_cols)),
        ResetColor,
    )
}

fn status_text(state: &AppState) -> String {
    let session = state.session.as_ref();
    let model = session.map(|s| s.model.as_str()).unwrap_or("unknown");
    let message_count = session.map(|s| s.message_count).unwrap_or(0);
    let busy = busy_status(state);

    format!(
        " {} · {} msgs · {}{}",
        model, message_count, state.directory_label, busy
    )
}

fn busy_status(state: &AppState) -> String {
    let Some(started) = state.turn.as_ref().map(|t| t.started_at) else {
        return String::new();
    };

    // Hide the queued counter while a takeover modal is up; the user's mental
    // model is "current turn paused on prompt", and the queue isn't growing.
    let queued = if state.modal.is_none() {
        state.turn.as_ref().map(|t| t.queued).unwrap_or(0)
    } else {
        0
    };
    let queued_str = if queued == 0 {
        String::new()
    } else {
        format!(" · {queued} queued")
    };
    let elapsed_ticks = (started.elapsed().as_millis() / 80) as usize;

    format!(
        " · {} {}s{}",
        theme::SPINNER[elapsed_ticks % theme::SPINNER.len()],
        started.elapsed().as_secs(),
        queued_str
    )
}
