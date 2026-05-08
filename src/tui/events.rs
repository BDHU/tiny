use crate::backend::BackendEvent;
use crate::tui::{
    permission::PermissionPromptModal,
    picker::SessionPicker,
    print::{self, Entry},
    prompt::Prompt,
    state::AppState,
};
use anyhow::Result;
use crossterm::{cursor::MoveTo, queue, terminal};
use std::io::Write;
use tiny::{Message, SessionMeta};

pub(crate) fn handle_backend_event<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    event: BackendEvent,
) -> Result<()> {
    match event {
        BackendEvent::Message(Message::User(_)) => {
            // Suppress duplicate - we printed it locally on Enter.
        }
        BackendEvent::Message(message) => {
            for entry in print::entries_from_message(message) {
                if matches!(entry, Entry::User(_) | Entry::Assistant(_)) {
                    state.record_chat_message();
                }
                emit_entry(out, prompt, &entry)?;
            }
        }
        BackendEvent::PermissionRequest { id, call } => {
            state.modal = Some(Box::new(PermissionPromptModal::new(id, call)));
        }
        BackendEvent::TurnStarted => {
            state.turn_started_by_backend();
        }
        BackendEvent::TurnError(error) => {
            emit_entry(out, prompt, &Entry::Error(error))?;
        }
        BackendEvent::TurnDone => {
            state.turn = None;
        }
        BackendEvent::SessionChanged { meta, history } => {
            handle_session_changed(out, prompt, state, meta, history)?;
        }
        BackendEvent::SessionsListed(Ok(sessions)) => {
            if sessions.is_empty() {
                emit_entry(
                    out,
                    prompt,
                    &Entry::Assistant("No saved sessions yet. Send a message to start one.".into()),
                )?;
            } else {
                let active_id = state.session.as_ref().map(|s| s.id.clone());
                state.modal = Some(Box::new(SessionPicker::new(sessions, active_id)));
            }
        }
        BackendEvent::SessionsListed(Err(error)) => {
            emit_entry(
                out,
                prompt,
                &Entry::Error(format!("list sessions: {error}")),
            )?;
        }
        BackendEvent::SessionError(error) => {
            emit_entry(out, prompt, &Entry::Error(error))?;
        }
    }
    Ok(())
}

pub(crate) fn emit_entry<W: Write>(out: &mut W, prompt: &mut Prompt, entry: &Entry) -> Result<()> {
    prompt.clear(out)?;
    print::print_entry(out, entry)?;
    out.flush()?;
    Ok(())
}

fn handle_session_changed<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    meta: SessionMeta,
    history: Vec<Message>,
) -> Result<()> {
    let is_initial = state.session.is_none();
    state.set_session(meta, &history);

    if !is_initial {
        separate_previous_session(out, prompt, history.is_empty())?;
    }
    print_history(out, prompt, history)
}

fn separate_previous_session<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    new_session_is_empty: bool,
) -> Result<()> {
    prompt.clear(out)?;
    if new_session_is_empty {
        scroll_visible_chat_into_scrollback(out)?;
    } else {
        print::print_separator(out)?;
    }
    out.flush()?;
    Ok(())
}

fn scroll_visible_chat_into_scrollback<W: Write>(out: &mut W) -> Result<()> {
    // /new: after prompt.clear(), emit enough newlines to push the visible
    // chat into scrollback before anchoring the next prompt at the top.
    let (_, term_rows) = terminal::size().unwrap_or((80, 24));
    for _ in 0..term_rows.saturating_sub(1) {
        out.write_all(b"\r\n")?;
    }
    queue!(out, MoveTo(0, 0))?;
    Ok(())
}

fn print_history<W: Write>(out: &mut W, prompt: &mut Prompt, history: Vec<Message>) -> Result<()> {
    if history.is_empty() {
        return Ok(());
    }

    prompt.clear(out)?;
    for message in history {
        for entry in print::entries_from_message(message) {
            print::print_entry(out, &entry)?;
        }
    }
    out.flush()?;
    Ok(())
}
