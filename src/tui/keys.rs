use crate::backend::{Backend, BackendCommand};
use crate::tui::{
    commands::{self, CommandAction},
    events::emit_entry,
    modal::ModalOutcome,
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

    if let Some(modal) = state.modal.as_mut() {
        match modal.handle_key(key, backend) {
            ModalOutcome::Continue => Ok(false),
            ModalOutcome::Close => {
                state.modal = None;
                Ok(false)
            }
            ModalOutcome::Quit => Ok(true),
        }
    } else {
        handle_main_key(out, prompt, state, backend, key, ctrl)
    }
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
            let next = state.palette_index as i32 + vertical_delta(key.code);
            state.palette_index = next.rem_euclid(len) as usize;
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

fn vertical_delta(code: KeyCode) -> i32 {
    if matches!(code, KeyCode::Up) {
        -1
    } else {
        1
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
