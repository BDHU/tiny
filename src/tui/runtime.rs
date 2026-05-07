use crate::backend::{self, Backend, BackendCommand, BackendEvent, PermissionId};
use crate::tui::{
    commands::{self, CommandAction},
    input::InputBuffer,
    picker::SessionPicker,
    print::{self, Entry},
    prompt::{Prompt, View},
    reader,
};
use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    event::{Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    queue, terminal,
    terminal::{Clear, ClearType},
};
use std::io::Write;
use std::mem;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny::{AgentConfig, Decision, Message, ToolCall};
use tokio::sync::mpsc;

const TICK_INTERVAL: Duration = Duration::from_millis(80);

enum Modal {
    None,
    SessionPicker(SessionPicker),
    PermissionPrompt(PermissionId, ToolCall),
}

struct TurnState {
    started_at: Instant,
    queued: usize,
}

struct Session {
    id: String,
    model: String,
    message_count: usize,
}

struct AppState {
    input: InputBuffer,
    palette_index: usize,
    session: Option<Session>,
    modal: Modal,
    turn: Option<TurnState>,
    directory_label: String,
    prompt: Prompt,
}

pub(crate) async fn run<W: Write>(
    out: &mut W,
    config: Arc<AgentConfig>,
    model: String,
) -> Result<()> {
    let mut state = AppState::new();
    let mut backend = backend::spawn(config, model.clone());
    let (reader_tx, mut reader_rx) = mpsc::unbounded_channel();
    let _reader = reader::spawn(reader_tx);
    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let cwd = std::env::current_dir().unwrap_or_default();
    print::print_intro(out, &model, &cwd.display().to_string())?;
    out.flush()?;
    render_screen(out, &mut state)?;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if state.turn.is_some() {
                    render_screen(out, &mut state)?;
                }
            }
            Some(event) = backend.events.recv() => {
                handle_backend_event(out, &mut state, event)?;
                while let Ok(event) = backend.events.try_recv() {
                    handle_backend_event(out, &mut state, event)?;
                }
                render_screen(out, &mut state)?;
            }
            event = reader_rx.recv() => {
                let Some(event) = event else { break };
                if handle_input_event(out, &mut state, &backend, event)? {
                    break;
                }
                render_screen(out, &mut state)?;
            }
            else => break,
        }
    }

    state.prompt.clear(out)?;
    out.flush()?;
    Ok(())
}

fn handle_backend_event<W: Write>(
    out: &mut W,
    state: &mut AppState,
    event: BackendEvent,
) -> Result<()> {
    match event {
        BackendEvent::Message(Message::User(_)) => {
            // Suppress duplicate - we printed it locally on Enter
        }
        BackendEvent::Message(message) => {
            for entry in print::entries_from_message(message) {
                if matches!(entry, Entry::User(_) | Entry::Assistant(_)) {
                    if let Some(session) = &mut state.session {
                        session.message_count += 1;
                    }
                }
                emit_entry(out, &mut state.prompt, &entry)?;
            }
        }
        BackendEvent::PermissionRequest { id, call } => {
            state.modal = Modal::PermissionPrompt(id, call);
        }
        BackendEvent::TurnStarted => {
            state.turn_started_by_backend();
        }
        BackendEvent::TurnError(error) => {
            emit_entry(out, &mut state.prompt, &Entry::Error(error))?;
        }
        BackendEvent::TurnDone => {
            state.turn = None;
        }
        BackendEvent::SessionChanged { meta, history } => {
            let is_initial = state.session.is_none();
            let message_count = history
                .iter()
                .filter(|m| matches!(m, Message::User(_) | Message::Assistant { .. }))
                .count();
            state.session = Some(Session {
                id: meta.id.0,
                model: meta.model,
                message_count,
            });
            if !is_initial {
                state.prompt.clear(out)?;
                print::print_separator(out)?;
                out.flush()?;
            }
            if !history.is_empty() {
                state.prompt.clear(out)?;
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
                    &mut state.prompt,
                    &Entry::Assistant(
                        "No saved sessions yet. Send a message to start one.".into(),
                    ),
                )?;
            } else {
                state.modal = Modal::SessionPicker(SessionPicker::new(sessions));
            }
        }
        BackendEvent::SessionsListed(Err(error)) => {
            emit_entry(
                out,
                &mut state.prompt,
                &Entry::Error(format!("list sessions: {error}")),
            )?;
        }
        BackendEvent::SessionError(error) => {
            emit_entry(out, &mut state.prompt, &Entry::Error(error))?;
        }
    }
    Ok(())
}

fn emit_entry<W: Write>(out: &mut W, prompt: &mut Prompt, entry: &Entry) -> Result<()> {
    prompt.clear(out)?;
    print::print_entry(out, entry)?;
    out.flush()?;
    Ok(())
}

fn handle_input_event<W: Write>(
    out: &mut W,
    state: &mut AppState,
    backend: &Backend,
    event: CtEvent,
) -> Result<bool> {
    match event {
        CtEvent::Key(key) if key.kind == KeyEventKind::Press => {
            handle_key(out, state, backend, key)
        }
        CtEvent::Paste(text) => {
            if matches!(state.modal, Modal::None) {
                state.input.insert_str(&text);
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_key<W: Write>(
    out: &mut W,
    state: &mut AppState,
    backend: &Backend,
    key: KeyEvent,
) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let is_busy = state.turn.is_some();

    match &mut state.modal {
        Modal::SessionPicker(picker) => match key.code {
            KeyCode::Char('c') if ctrl => Ok(true),
            KeyCode::Up | KeyCode::Down => {
                let delta = if matches!(key.code, KeyCode::Up) { -1 } else { 1 };
                picker.move_by(delta);
                Ok(false)
            }
            KeyCode::Enter => {
                if let Modal::SessionPicker(picker) =
                    mem::replace(&mut state.modal, Modal::None)
                {
                    if let Some(id) = picker.into_selected_id() {
                        let _ = backend.commands.send(BackendCommand::SwitchSession(id));
                    }
                }
                Ok(false)
            }
            KeyCode::Esc => {
                state.modal = Modal::None;
                Ok(false)
            }
            _ => Ok(false),
        },
        Modal::PermissionPrompt(_, _) => {
            let decision = match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => Some(Decision::Allow),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    Some(Decision::Deny("denied by user".into()))
                }
                KeyCode::Char('c') if ctrl => return Ok(true),
                _ => None,
            };
            if let Some(decision) = decision {
                if let Modal::PermissionPrompt(perm_id, _) =
                    mem::replace(&mut state.modal, Modal::None)
                {
                    let _ = backend.commands.send(BackendCommand::PermissionDecision {
                        id: perm_id,
                        decision,
                    });
                }
            }
            Ok(false)
        }
        Modal::None => {
            // Slash-command palette
            let palette = commands::palette_matches(state.input.as_str());
            if !palette.is_empty() {
                match key.code {
                    KeyCode::Up | KeyCode::Down => {
                        let len = palette.len() as i32;
                        let delta = if matches!(key.code, KeyCode::Up) { -1 } else { 1 };
                        let next = (state.palette_index as i32 + delta).rem_euclid(len);
                        state.palette_index = next as usize;
                        return Ok(false);
                    }
                    KeyCode::Tab => {
                        let selected = palette[state.palette_index.min(palette.len() - 1)];
                        state.input.clear();
                        state.input.insert_str(&format!("/{} ", selected.name));
                        state.palette_index = 0;
                        return Ok(false);
                    }
                    KeyCode::Enter => {
                        let selected_name =
                            palette[state.palette_index.min(palette.len() - 1)].name;
                        state.input.clear();
                        state.palette_index = 0;
                        return dispatch_command(out, state, backend, selected_name, is_busy);
                    }
                    _ => {}
                }
            }

            match key.code {
                KeyCode::Char('c') if ctrl => Ok(true),
                KeyCode::Char('l') if ctrl => {
                    state.prompt.clear(out)?;
                    queue!(out, Clear(ClearType::All), MoveTo(0, 0))?;
                    out.flush()?;
                    Ok(false)
                }
                KeyCode::Enter if !state.input.is_blank() => {
                    let input = state.input.clear();
                    state.palette_index = 0;
                    if let Some(rest) = input.strip_prefix('/') {
                        let rest = rest.to_string();
                        dispatch_command(out, state, backend, &rest, is_busy)
                    } else {
                        emit_entry(out, &mut state.prompt, &Entry::User(input.clone()))?;
                        if let Some(session) = &mut state.session {
                            session.message_count += 1;
                        }
                        state.turn_submitted();
                        let _ = backend.commands.send(BackendCommand::Submit(input));
                        Ok(false)
                    }
                }
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
    }
}

fn dispatch_command<W: Write>(
    out: &mut W,
    state: &mut AppState,
    backend: &Backend,
    input: &str,
    is_busy: bool,
) -> Result<bool> {
    match commands::dispatch(input, is_busy) {
        Ok(CommandAction::NewSession) => {
            let _ = backend.commands.send(BackendCommand::NewSession);
            Ok(false)
        }
        Ok(CommandAction::OpenSessions) => {
            let _ = backend.commands.send(BackendCommand::ListSessions);
            Ok(false)
        }
        Ok(CommandAction::ShowHelp) => {
            emit_entry(
                out,
                &mut state.prompt,
                &Entry::Assistant(commands::help_text()),
            )?;
            Ok(false)
        }
        Ok(CommandAction::Quit) => Ok(true),
        Err(error) => {
            emit_entry(out, &mut state.prompt, &Entry::Error(error.to_string()))?;
            Ok(false)
        }
    }
}

fn render_screen<W: Write>(out: &mut W, state: &mut AppState) -> Result<()> {
    let term_size = terminal::size().unwrap_or((80, 24));

    let model = state
        .session
        .as_ref()
        .map(|s| s.model.as_str())
        .unwrap_or("unknown");
    let message_count = state.session.as_ref().map(|s| s.message_count).unwrap_or(0);
    let active_session_id = state.session.as_ref().map(|s| s.id.as_str());

    let (pending_call, queued) = match &state.modal {
        Modal::PermissionPrompt(_, call) => (Some(call), 0),
        Modal::SessionPicker(_) => (None, 0),
        Modal::None => {
            let queued = state.turn.as_ref().map(|t| t.queued).unwrap_or(0);
            (None, queued)
        }
    };

    let view = View {
        input: &state.input,
        palette_index: state.palette_index,
        picker: match &state.modal {
            Modal::SessionPicker(p) => Some(p),
            _ => None,
        },
        pending_call,
        turn_started_at: state.turn.as_ref().map(|t| t.started_at),
        queued,
        model,
        directory_label: &state.directory_label,
        message_count,
        active_session_id,
        terminal_size: term_size,
    };
    state.prompt.render(out, &view)?;
    Ok(())
}

impl AppState {
    fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let directory_label = cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| cwd.display().to_string());
        Self {
            input: InputBuffer::default(),
            palette_index: 0,
            session: None,
            modal: Modal::None,
            turn: None,
            directory_label,
            prompt: Prompt::default(),
        }
    }

    /// Local input was submitted: start a turn or queue behind the current one.
    fn turn_submitted(&mut self) {
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
    fn turn_started_by_backend(&mut self) {
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
