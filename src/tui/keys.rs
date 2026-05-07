use crate::backend::{Backend, BackendCommand};
use crate::tui::{
    commands::{self, CommandAction},
    events::emit_entry,
    print::Entry,
    prompt::Prompt,
    state::{AppState, Modal},
};
use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    event::{Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    queue,
    terminal::{Clear, ClearType},
};
use std::io::Write;
use tiny::Decision;

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
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match state.modal.as_mut() {
        Some(Modal::SessionPicker(picker)) => match key.code {
            KeyCode::Char('c') if ctrl => Ok(true),
            KeyCode::Up | KeyCode::Down => {
                let delta = if matches!(key.code, KeyCode::Up) {
                    -1
                } else {
                    1
                };
                picker.move_by(delta);
                Ok(false)
            }
            KeyCode::Enter => {
                if let Some(Modal::SessionPicker(picker)) = state.modal.take() {
                    if let Some(id) = picker.into_selected_id() {
                        let _ = backend.commands.send(BackendCommand::SwitchSession(id));
                    }
                }
                Ok(false)
            }
            KeyCode::Esc => {
                state.modal = None;
                Ok(false)
            }
            _ => Ok(false),
        },
        Some(Modal::PermissionPrompt(_, _)) => handle_permission_key(state, backend, key, ctrl),
        None => handle_main_key(out, prompt, state, backend, key, ctrl),
    }
}

fn handle_permission_key(
    state: &mut AppState,
    backend: &Backend,
    key: KeyEvent,
    ctrl: bool,
) -> Result<bool> {
    let decision = match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(Decision::Allow),
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            Some(Decision::Deny("denied by user".into()))
        }
        KeyCode::Char('c') if ctrl => return Ok(true),
        _ => None,
    };

    if let Some(decision) = decision {
        if let Some(Modal::PermissionPrompt(perm_id, _)) = state.modal.take() {
            let _ = backend.commands.send(BackendCommand::PermissionDecision {
                id: perm_id,
                decision,
            });
        }
    }
    Ok(false)
}

fn handle_main_key<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
    key: KeyEvent,
    ctrl: bool,
) -> Result<bool> {
    if let Some(should_quit) = handle_palette_key(out, prompt, state, backend, key)? {
        return Ok(should_quit);
    }

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
            state.palette_index = 0;
            Ok(false)
        }
        KeyCode::Backspace => {
            state.input.backspace();
            state.palette_index = 0;
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
            state.palette_index = 0;
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_palette_key<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
    key: KeyEvent,
) -> Result<Option<bool>> {
    let palette = commands::palette_matches(state.input.as_str());
    if palette.is_empty() {
        return Ok(None);
    }

    match key.code {
        KeyCode::Up | KeyCode::Down => {
            let len = palette.len() as i32;
            let delta = if matches!(key.code, KeyCode::Up) {
                -1
            } else {
                1
            };
            let next = (state.palette_index as i32 + delta).rem_euclid(len);
            state.palette_index = next as usize;
            Ok(Some(false))
        }
        KeyCode::Tab => {
            let selected = palette[state.palette_index.min(palette.len() - 1)];
            state.input.clear();
            state.input.insert_str(&format!("/{} ", selected.name));
            state.palette_index = 0;
            Ok(Some(false))
        }
        KeyCode::Enter => {
            let selected_name = palette[state.palette_index.min(palette.len() - 1)].name;
            state.input.clear();
            state.palette_index = 0;
            dispatch_command(out, prompt, state, backend, selected_name).map(Some)
        }
        _ => Ok(None),
    }
}

fn submit_input<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut AppState,
    backend: &Backend,
) -> Result<bool> {
    let input = state.input.clear();
    state.palette_index = 0;

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
    match commands::dispatch(input, state.is_busy()) {
        Ok(CommandAction::NewSession) => {
            let _ = backend.commands.send(BackendCommand::NewSession);
            Ok(false)
        }
        Ok(CommandAction::OpenSessions) => {
            let _ = backend.commands.send(BackendCommand::ListSessions);
            Ok(false)
        }
        Ok(CommandAction::ShowHelp) => {
            emit_entry(out, prompt, &Entry::Assistant(commands::help_text()))?;
            Ok(false)
        }
        Ok(CommandAction::Quit) => Ok(true),
        Err(error) => {
            emit_entry(out, prompt, &Entry::Error(error.to_string()))?;
            Ok(false)
        }
    }
}
