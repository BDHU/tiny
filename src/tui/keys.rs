use crate::backend::{Backend, BackendCommand};
use crate::tui::{
    commands,
    events::emit_entry,
    modal::{KeyDispatch, ModalOutcome},
    print::Entry,
    prompt::Prompt,
    state::AppState,
};
use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    event::{Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    queue,
    terminal::{Clear, ClearType},
};
use std::io::Write;

pub(crate) fn handle_input_event<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
    event: CtEvent,
) -> Result<bool> {
    match event {
        CtEvent::Key(key) if key.kind == KeyEventKind::Press => {
            handle_key(out, prompt, state, backend, key)
        }
        CtEvent::Paste(text) => {
            // Paste is for the input buffer; takeover modals own input, so
            // suppress while one is open.
            if state.modal.is_none() {
                state.input.insert_str(&text);
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_key<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
    key: KeyEvent,
) -> Result<bool> {
    // 1. Takeover modal claims everything.
    if let Some(quit) = dispatch_takeover(out, prompt, state, backend, key)? {
        return Ok(quit);
    }
    // 2. Overlay (palette) gets first dibs but can pass through.
    if let Some(quit) = dispatch_overlay(out, prompt, state, backend, key)? {
        return Ok(quit);
    }
    // 3. Default input handling.
    handle_main_key(out, prompt, state, backend, key)
}

fn dispatch_takeover<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
    key: KeyEvent,
) -> Result<Option<bool>> {
    let Some(mut modal) = state.modal.take() else {
        return Ok(None);
    };
    let dispatch = modal.handle_key(key, state);
    match dispatch {
        KeyDispatch::PassThrough => {
            // No takeover modal currently passes through, but if one did we
            // reinstall it and let the rest of the ladder run.
            state.modal = Some(modal);
            Ok(None)
        }
        KeyDispatch::Consumed(outcome) => {
            let (quit, keep) = apply_outcome(out, prompt, state, backend, outcome)?;
            if keep {
                state.modal = Some(modal);
            }
            Ok(Some(quit))
        }
    }
}

fn dispatch_overlay<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
    key: KeyEvent,
) -> Result<Option<bool>> {
    let Some(mut overlay) = state.overlay.take() else {
        return Ok(None);
    };
    let dispatch = overlay.handle_key(key, state);
    // Overlays are permanent — always reinstall regardless of outcome.
    state.overlay = Some(overlay);
    match dispatch {
        KeyDispatch::PassThrough => Ok(None),
        KeyDispatch::Consumed(outcome) => {
            let (quit, _keep) = apply_outcome(out, prompt, state, backend, outcome)?;
            Ok(Some(quit))
        }
    }
}

/// Apply a modal outcome's side effects. Returns `(should_quit, keep_modal)`.
/// `keep_modal` is meaningful for takeover modals; overlays ignore it.
fn apply_outcome<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    _state: &mut AppState,
    backend: &Backend,
    outcome: ModalOutcome,
) -> Result<(bool, bool)> {
    match outcome {
        ModalOutcome::Continue => Ok((false, true)),
        ModalOutcome::Close => Ok((false, false)),
        ModalOutcome::Quit => Ok((true, false)),
        ModalOutcome::Emit(cmd) => {
            let _ = backend.commands.send(cmd);
            Ok((false, true))
        }
        ModalOutcome::EmitAndClose(cmd) => {
            let _ = backend.commands.send(cmd);
            Ok((false, false))
        }
        ModalOutcome::Print(entry) => {
            emit_entry(out, prompt, &entry)?;
            Ok((false, true))
        }
    }
}

fn handle_main_key<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
    key: KeyEvent,
) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => Ok(true),
        KeyCode::Char('l') if ctrl => {
            prompt.clear(out)?;
            queue!(out, Clear(ClearType::All), MoveTo(0, 0))?;
            out.flush()?;
            Ok(false)
        }
        KeyCode::Enter if !state.input.is_blank() => submit_input(out, prompt, state, backend),
        KeyCode::Char(c) => {
            state.input.insert_char(c);
            Ok(false)
        }
        KeyCode::Backspace => {
            state.input.backspace();
            Ok(false)
        }
        KeyCode::Left => {
            state.input.move_left();
            Ok(false)
        }
        KeyCode::Right => {
            state.input.move_right();
            Ok(false)
        }
        KeyCode::Esc => {
            state.input.clear();
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn submit_input<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
) -> Result<bool> {
    let input = state.input.clear();

    if let Some(rest) = input.strip_prefix('/') {
        return dispatch_command(out, prompt, state, backend, rest);
    }

    emit_entry(out, prompt, &Entry::User(input.clone()))?;
    state.record_chat_message();
    state.turn_submitted();
    let _ = backend.commands.send(BackendCommand::Submit(input));
    Ok(false)
}

fn dispatch_command<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
    input: &str,
) -> Result<bool> {
    let outcome = commands::dispatch_outcome(input, state.is_busy());
    let (quit, _keep) = apply_outcome(out, prompt, state, backend, outcome)?;
    Ok(quit)
}
