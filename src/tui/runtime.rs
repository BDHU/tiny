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
    event::{Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind},
    queue, terminal,
    terminal::{Clear, ClearType},
};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny::{AgentConfig, Decision, Message, ToolCall};
use tokio::sync::mpsc;

const TICK_INTERVAL: Duration = Duration::from_millis(80);

struct State {
    input: InputBuffer,
    model: String,
    directory: String,
    directory_label: String,
    session_id: Option<String>,
    busy: bool,
    busy_started_at: Option<Instant>,
    tick: usize,
    queued: usize,
    pending: Option<PendingPermission>,
    picker: Option<SessionPicker>,
    palette_index: usize,
    message_count: usize,
}

struct PendingPermission {
    id: PermissionId,
    call: ToolCall,
}

enum Outcome {
    Continue,
    Quit,
}

pub(crate) async fn run<W: Write>(
    out: &mut W,
    config: Arc<AgentConfig>,
    model: String,
) -> Result<()> {
    let mut state = State::new(model.clone());
    let mut prompt = Prompt::default();
    let mut backend = backend::spawn(config, model);
    let (reader_tx, mut reader_rx) = mpsc::unbounded_channel();
    let _reader = reader::spawn(reader_tx);
    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    print::print_intro(out, &state.model, &state.directory)?;
    out.flush()?;
    redraw(out, &mut prompt, &state)?;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if state.busy {
                    state.tick = state.tick.wrapping_add(1);
                    redraw(out, &mut prompt, &state)?;
                }
            }
            Some(event) = backend.events.recv() => {
                handle_backend(out, &mut prompt, &mut state, event)?;
                while let Ok(event) = backend.events.try_recv() {
                    handle_backend(out, &mut prompt, &mut state, event)?;
                }
                redraw(out, &mut prompt, &state)?;
            }
            Some(event) = reader_rx.recv() => {
                let outcome = handle_reader(out, &mut prompt, &mut state, &backend, event)?;
                if let Outcome::Quit = outcome {
                    break;
                }
                redraw(out, &mut prompt, &state)?;
            }
            else => break,
        }
    }

    prompt.clear(out)?;
    out.flush()?;
    Ok(())
}

fn handle_backend<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut State,
    event: BackendEvent,
) -> Result<()> {
    match event {
        // The agent records the user message as the first event of every
        // turn. We already printed it locally on Enter; suppress the duplicate.
        BackendEvent::Message(Message::User(_)) => {}
        BackendEvent::Message(message) => {
            for entry in print::entries_from_message(message) {
                if matches!(entry, Entry::User(_) | Entry::Assistant(_)) {
                    state.message_count += 1;
                }
                emit_entry(out, prompt, &entry)?;
            }
        }
        BackendEvent::PermissionRequest { id, call } => {
            state.pending = Some(PendingPermission { id, call });
        }
        BackendEvent::TurnStarted => {
            if !state.busy && state.queued > 0 {
                state.queued -= 1;
            }
            state.busy = true;
            state.busy_started_at = Some(Instant::now());
        }
        BackendEvent::TurnError(error) => {
            emit_entry(out, prompt, &Entry::Error(error))?;
        }
        BackendEvent::TurnDone => {
            state.busy = false;
            state.busy_started_at = None;
        }
        BackendEvent::SessionChanged { meta, history } => {
            let initial = state.session_id.is_none();
            state.session_id = Some(meta.id.0);
            state.model = meta.model;
            state.message_count = history
                .iter()
                .filter(|m| matches!(m, Message::User(_) | Message::Assistant { .. }))
                .count();
            if !initial {
                emit_separator(out, prompt)?;
            }
            if !history.is_empty() {
                replay_history(out, prompt, history)?;
            }
        }
        BackendEvent::SessionsListed(result) => match result {
            Ok(sessions) if sessions.is_empty() => {
                emit_entry(
                    out,
                    prompt,
                    &Entry::Assistant(
                        "No saved sessions yet. Send a message to start one.".into(),
                    ),
                )?;
            }
            Ok(sessions) => {
                state.picker = Some(SessionPicker::new(sessions));
            }
            Err(error) => {
                emit_entry(out, prompt, &Entry::Error(format!("list sessions: {error}")))?;
            }
        },
        BackendEvent::SessionError(error) => {
            emit_entry(out, prompt, &Entry::Error(error))?;
        }
    }
    Ok(())
}

fn replay_history<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    history: Vec<Message>,
) -> Result<()> {
    prompt.clear(out)?;
    for message in history {
        for entry in print::entries_from_message(message) {
            print::print_entry(out, &entry)?;
        }
    }
    out.flush()?;
    Ok(())
}

fn emit_separator<W: Write>(out: &mut W, prompt: &mut Prompt) -> Result<()> {
    prompt.clear(out)?;
    print::print_separator(out)?;
    out.flush()?;
    Ok(())
}

fn queue_clear_screen<W: Write>(out: &mut W) -> io::Result<()> {
    queue!(out, Clear(ClearType::All), MoveTo(0, 0))
}

fn emit_entry<W: Write>(out: &mut W, prompt: &mut Prompt, entry: &Entry) -> Result<()> {
    prompt.clear(out)?;
    print::print_entry(out, entry)?;
    out.flush()?;
    Ok(())
}

fn handle_reader<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut State,
    backend: &Backend,
    event: reader::ReaderEvent,
) -> Result<Outcome> {
    match event {
        reader::ReaderEvent::Terminal(CtEvent::Key(key)) if key.kind == KeyEventKind::Press => {
            handle_key(out, prompt, state, backend, key)
        }
        reader::ReaderEvent::Terminal(CtEvent::Paste(text)) => {
            if state.pending.is_none() {
                state.input.insert_str(&text);
            }
            Ok(Outcome::Continue)
        }
        reader::ReaderEvent::Terminal(CtEvent::Mouse(mouse)) => {
            // Mouse scroll handed off to terminal scrollback; nothing to do.
            let _ = matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            );
            Ok(Outcome::Continue)
        }
        reader::ReaderEvent::Terminal(_) => Ok(Outcome::Continue),
        reader::ReaderEvent::Error(error) => {
            emit_entry(out, prompt, &Entry::Error(error))?;
            Ok(Outcome::Continue)
        }
    }
}

fn handle_key<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut State,
    backend: &Backend,
    key: KeyEvent,
) -> Result<Outcome> {
    if state.pending.is_some() {
        return handle_permission_key(state, backend, key);
    }
    if state.picker.is_some() {
        return handle_picker_key(state, backend, key);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if let Some(outcome) = handle_palette_key(out, prompt, state, backend, key)? {
        return Ok(outcome);
    }

    match key.code {
        KeyCode::Char('c') if ctrl => Ok(Outcome::Quit),
        KeyCode::Char('l') if ctrl => {
            prompt.clear(out)?;
            queue_clear_screen(out)?;
            out.flush()?;
            Ok(Outcome::Continue)
        }
        KeyCode::Enter if !state.input.is_blank() => submit_input(out, prompt, state, backend),
        KeyCode::Char(c) => {
            state.input.insert_char(c);
            state.palette_index = 0;
            Ok(Outcome::Continue)
        }
        KeyCode::Backspace => {
            state.input.backspace();
            state.palette_index = 0;
            Ok(Outcome::Continue)
        }
        KeyCode::Left => {
            state.input.move_left();
            Ok(Outcome::Continue)
        }
        KeyCode::Right => {
            state.input.move_right();
            Ok(Outcome::Continue)
        }
        KeyCode::Esc => {
            state.input.clear();
            state.palette_index = 0;
            Ok(Outcome::Continue)
        }
        _ => Ok(Outcome::Continue),
    }
}

fn handle_palette_key<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut State,
    backend: &Backend,
    key: KeyEvent,
) -> Result<Option<Outcome>> {
    if !commands::palette_active(state.input.as_str()) {
        return Ok(None);
    }

    let matches = commands::palette_matches(state.input.as_str());
    if matches.is_empty() {
        return match key.code {
            KeyCode::Up | KeyCode::Down | KeyCode::Tab => Ok(Some(Outcome::Continue)),
            _ => Ok(None),
        };
    }

    match key.code {
        KeyCode::Up | KeyCode::Down => {
            let len = matches.len() as i32;
            let delta = if matches!(key.code, KeyCode::Up) { -1 } else { 1 };
            let next = (state.palette_index as i32 + delta).rem_euclid(len);
            state.palette_index = next as usize;
            Ok(Some(Outcome::Continue))
        }
        KeyCode::Tab => {
            let selected = matches[state.palette_index.min(matches.len() - 1)];
            state.input.clear();
            state.input.insert_str(&format!("/{} ", selected.name));
            state.palette_index = 0;
            Ok(Some(Outcome::Continue))
        }
        KeyCode::Enter => {
            let selected_name = matches[state.palette_index.min(matches.len() - 1)].name;
            state.input.clear();
            state.palette_index = 0;
            Ok(Some(dispatch_command(out, prompt, state, backend, selected_name)?))
        }
        _ => Ok(None),
    }
}

fn submit_input<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut State,
    backend: &Backend,
) -> Result<Outcome> {
    let input = state.input.clear();
    state.palette_index = 0;
    if let Some(rest) = input.strip_prefix('/') {
        return dispatch_command(out, prompt, state, backend, rest);
    }
    emit_entry(out, prompt, &Entry::User(input.clone()))?;
    state.message_count += 1;
    if state.busy {
        state.queued += 1;
    } else {
        state.busy = true;
        state.busy_started_at = Some(Instant::now());
    }
    let _ = backend.commands.send(BackendCommand::Submit(input));
    Ok(Outcome::Continue)
}

fn dispatch_command<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &mut State,
    backend: &Backend,
    input: &str,
) -> Result<Outcome> {
    match commands::dispatch(input, state.busy) {
        Ok(CommandAction::NewSession) => {
            let _ = backend.commands.send(BackendCommand::NewSession);
            Ok(Outcome::Continue)
        }
        Ok(CommandAction::OpenSessions) => {
            let _ = backend.commands.send(BackendCommand::ListSessions);
            Ok(Outcome::Continue)
        }
        Ok(CommandAction::ShowHelp) => {
            emit_entry(out, prompt, &Entry::Assistant(commands::help_text()))?;
            Ok(Outcome::Continue)
        }
        Ok(CommandAction::Quit) => Ok(Outcome::Quit),
        Err(error) => {
            emit_entry(out, prompt, &Entry::Error(error.to_string()))?;
            Ok(Outcome::Continue)
        }
    }
}

fn handle_picker_key(
    state: &mut State,
    backend: &Backend,
    key: KeyEvent,
) -> Result<Outcome> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => Ok(Outcome::Quit),
        KeyCode::Up | KeyCode::Down => {
            if let Some(picker) = state.picker.as_mut() {
                let delta = if matches!(key.code, KeyCode::Up) { -1 } else { 1 };
                picker.move_by(delta);
            }
            Ok(Outcome::Continue)
        }
        KeyCode::Enter => {
            if let Some(id) = state.picker.take().and_then(|p| p.into_selected_id()) {
                let _ = backend.commands.send(BackendCommand::SwitchSession(id));
            }
            Ok(Outcome::Continue)
        }
        KeyCode::Esc => {
            state.picker = None;
            Ok(Outcome::Continue)
        }
        _ => Ok(Outcome::Continue),
    }
}

fn handle_permission_key(
    state: &mut State,
    backend: &Backend,
    key: KeyEvent,
) -> Result<Outcome> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let decision = match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Decision::Allow,
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            Decision::Deny("denied by user".into())
        }
        KeyCode::Char('c') if ctrl => return Ok(Outcome::Quit),
        _ => return Ok(Outcome::Continue),
    };

    if let Some(pending) = state.pending.take() {
        let _ = backend.commands.send(BackendCommand::PermissionDecision {
            id: pending.id,
            decision,
        });
    }
    Ok(Outcome::Continue)
}

fn redraw<W: Write>(out: &mut W, prompt: &mut Prompt, state: &State) -> Result<()> {
    let term_size = terminal::size().unwrap_or((80, 24));
    let view = View {
        input: &state.input,
        palette_index: state.palette_index,
        picker: state.picker.as_ref(),
        pending: state.pending.as_ref().map(|p| &p.call),
        busy: state.busy,
        busy_started_at: state.busy_started_at,
        tick: state.tick,
        queued: state.queued,
        model: &state.model,
        directory_label: &state.directory_label,
        message_count: state.message_count,
        active_session_id: state.session_id.as_deref(),
        terminal_size: term_size,
    };
    prompt.render(out, &view)?;
    Ok(())
}

impl State {
    fn new(model: String) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let directory = cwd.display().to_string();
        let directory_label = cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| directory.clone());
        Self {
            input: InputBuffer::default(),
            model,
            directory,
            directory_label,
            session_id: None,
            busy: false,
            busy_started_at: None,
            tick: 0,
            queued: 0,
            pending: None,
            picker: None,
            palette_index: 0,
            message_count: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests for InputBuffer/commands live in their own modules; the runtime
    // is driven by integration through stdin/stdout, so it is exercised at
    // the binary level rather than here.
}
