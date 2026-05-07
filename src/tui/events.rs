use crate::backend::BackendEvent;
use crate::tui::{
    picker::SessionPicker,
    print::{self, Entry},
    prompt::Prompt,
    state::{AppState, Modal},
};
use anyhow::Result;
use std::io::Write;
use tiny::Message;

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
            state.modal = Some(Modal::PermissionPrompt(id, call));
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
            let is_initial = !state.has_session();
            state.set_session(meta, &history);
            if !is_initial {
                prompt.clear(out)?;
                print::print_separator(out)?;
                out.flush()?;
            }
            if !history.is_empty() {
                prompt.clear(out)?;
                for message in history {
                    for entry in print::entries_from_message(message) {
                        print::print_entry(out, &entry)?;
                    }
                }
                out.flush()?;
            }
        }
        BackendEvent::SessionsListed(Ok(sessions)) => {
            if sessions.is_empty() {
                emit_entry(
                    out,
                    prompt,
                    &Entry::Assistant("No saved sessions yet. Send a message to start one.".into()),
                )?;
            } else {
                state.modal = Some(Modal::SessionPicker(SessionPicker::new(sessions)));
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
