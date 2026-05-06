use crate::backend::{BackendCommand, PermissionId};
use crate::tui::commands;
use crate::tui::input::InputBuffer;
use crate::tui::picker::SessionPicker;
use crate::tui::scroll::ScrollState;
use crate::tui::transcript::{entries_from_message, Entry, Transcript};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;
use tiny::{Decision, Message, SessionMeta, ToolCall};

pub(crate) struct PendingPermission {
    pub(crate) id: PermissionId,
    pub(crate) call: ToolCall,
}

#[derive(Default)]
pub(crate) struct TurnState {
    pub(crate) busy: bool,
    pub(crate) started_at: Option<Instant>,
}

pub(crate) struct SessionInfo {
    pub(crate) model: String,
    pub(crate) directory: String,
    pub(crate) directory_label: String,
    pub(crate) id: Option<String>,
}

pub(crate) struct State {
    pub(crate) input: InputBuffer,
    pub(crate) transcript: Transcript,
    pub(crate) scroll: ScrollState,
    pub(crate) tick: usize,
    pub(crate) session: SessionInfo,
    pub(crate) turn: TurnState,
    pub(crate) queued: usize,
    pub(crate) pending: Option<PendingPermission>,
    pub(crate) palette_index: usize,
    pub(crate) picker: Option<SessionPicker>,
}

pub(crate) enum UiEvent {
    Key(KeyEvent),
    Paste(String),
    Scroll(i16),
    Viewport {
        width: u16,
        height: u16,
    },
    Entry(Entry),
    PermissionRequest {
        id: PermissionId,
        call: ToolCall,
    },
    TurnStarted,
    TurnError(String),
    TurnDone,
    Tick,
    SessionChanged {
        meta: SessionMeta,
        history: Vec<Message>,
    },
    SessionsListed(Result<Vec<SessionMeta>, String>),
}

pub(crate) enum Effect {
    Quit,
    Redraw,
    Backend(BackendCommand),
}

impl State {
    pub(crate) fn new(model: String) -> Self {
        Self {
            input: InputBuffer::default(),
            transcript: Transcript::default(),
            scroll: ScrollState::following_tail(),
            tick: 0,
            session: SessionInfo::new(model),
            turn: TurnState::default(),
            queued: 0,
            pending: None,
            palette_index: 0,
            picker: None,
        }
    }

    fn begin_turn(&mut self) {
        self.turn.busy = true;
        self.turn.started_at = Some(Instant::now());
    }

    fn begin_or_queue_turn(&mut self) {
        if self.turn.busy {
            self.queued += 1;
        } else {
            self.begin_turn();
        }
    }

    fn resize_viewport(&mut self, width: u16, height: u16) {
        self.transcript.resize(width);
        self.scroll.set_content_size(height, self.content_height());
    }

    fn content_height(&self) -> u16 {
        let busy_height = if self.turn.busy { 2 } else { 0 };
        self.transcript.height().saturating_add(busy_height)
    }

    fn push_entry(&mut self, entry: Entry) {
        self.transcript.push(entry);
        self.scroll.content_changed();
    }

    fn submit_input(&mut self) -> Option<Effect> {
        let input = self.input.clear();
        if let Some(rest) = input.strip_prefix('/') {
            return self.dispatch_command(rest);
        }
        self.push_entry(Entry::User(input.clone()));
        self.scroll.follow_tail();
        self.begin_or_queue_turn();
        Some(Effect::Backend(BackendCommand::Submit(input)))
    }

    fn dispatch_command(&mut self, input: &str) -> Option<Effect> {
        match commands::dispatch(input, self.turn.busy) {
            Ok(commands::CommandAction::NewSession) => {
                Some(Effect::Backend(BackendCommand::NewSession))
            }
            Ok(commands::CommandAction::OpenSessions) => {
                Some(Effect::Backend(BackendCommand::ListSessions))
            }
            Ok(commands::CommandAction::ShowHelp) => {
                self.push_entry(Entry::Assistant(commands::help_text()));
                None
            }
            Ok(commands::CommandAction::Quit) => Some(Effect::Quit),
            Err(error) => {
                self.push_entry(Entry::Error(error.to_string()));
                None
            }
        }
    }

    fn replace_session(&mut self, meta: SessionMeta, history: Vec<Message>) {
        self.session.id = Some(meta.id.0);
        self.session.model = meta.model;
        self.transcript.clear();
        for message in history {
            for entry in entries_from_message(message) {
                self.transcript.push(entry);
            }
        }
        self.scroll.follow_tail();
    }

    fn show_session_picker(&mut self, result: Result<Vec<SessionMeta>, String>) {
        let sessions = match result {
            Ok(sessions) => sessions,
            Err(error) => {
                self.push_entry(Entry::Error(format!("list sessions: {error}")));
                return;
            }
        };

        if sessions.is_empty() {
            self.push_entry(Entry::Assistant(
                "No saved sessions yet. Send a message to start one.".into(),
            ));
            return;
        }

        self.picker = Some(SessionPicker::new(sessions));
    }
}

impl SessionInfo {
    fn new(model: String) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let directory = cwd.display().to_string();
        let directory_label = cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| directory.clone());

        Self {
            model,
            directory,
            directory_label,
            id: None,
        }
    }
}

pub(crate) fn update(state: &mut State, event: UiEvent) -> Option<Effect> {
    match event {
        UiEvent::Key(key) => return handle_key(state, key),
        UiEvent::Paste(text) => {
            if state.pending.is_none() {
                state.input.insert_str(&text);
            }
        }
        UiEvent::Scroll(lines) => state.scroll.scroll_by(lines),
        UiEvent::Viewport { width, height } => state.resize_viewport(width, height),
        UiEvent::Entry(entry) => state.push_entry(entry),
        UiEvent::PermissionRequest { id, call } => {
            state.pending = Some(PendingPermission { id, call });
        }
        UiEvent::TurnStarted => {
            if !state.turn.busy && state.queued > 0 {
                state.queued -= 1;
            }
            state.begin_turn();
        }
        UiEvent::TurnError(error) => state.push_entry(Entry::Error(error)),
        UiEvent::TurnDone => {
            state.turn.busy = false;
            state.turn.started_at = None;
        }
        UiEvent::Tick => state.tick = state.tick.wrapping_add(1),
        UiEvent::SessionChanged { meta, history } => state.replace_session(meta, history),
        UiEvent::SessionsListed(result) => state.show_session_picker(result),
    }
    None
}

fn handle_key(state: &mut State, key: KeyEvent) -> Option<Effect> {
    if state.pending.is_some() {
        return handle_permission_key(state, key);
    }
    if state.picker.is_some() {
        return handle_picker_key(state, key);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if let Some(effect) = handle_palette_key(state, key) {
        return effect;
    }

    match key.code {
        KeyCode::Char('c') if ctrl => Some(Effect::Quit),
        KeyCode::Char('l') if ctrl => Some(Effect::Redraw),
        KeyCode::Enter if !state.input.is_blank() => state.submit_input(),
        KeyCode::Char(c) => {
            state.input.insert_char(c);
            state.palette_index = 0;
            None
        }
        KeyCode::Backspace => {
            state.input.backspace();
            state.palette_index = 0;
            None
        }
        KeyCode::Left => {
            state.input.move_left();
            None
        }
        KeyCode::Right => {
            state.input.move_right();
            None
        }
        KeyCode::Esc => {
            state.input.clear();
            state.palette_index = 0;
            None
        }
        KeyCode::PageUp => {
            state.scroll.scroll_by(-10);
            None
        }
        KeyCode::PageDown => {
            state.scroll.scroll_by(10);
            None
        }
        KeyCode::Up => {
            state.scroll.scroll_by(-1);
            None
        }
        KeyCode::Down => {
            state.scroll.scroll_by(1);
            None
        }
        _ => None,
    }
}

fn handle_palette_key(state: &mut State, key: KeyEvent) -> Option<Option<Effect>> {
    if !commands::palette_active(state.input.as_str()) {
        return None;
    }

    let matches = commands::palette_matches(state.input.as_str());
    if matches.is_empty() {
        return match key.code {
            KeyCode::Up | KeyCode::Down | KeyCode::Tab => Some(None),
            KeyCode::Enter => Some(state.submit_input()),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Up => {
            move_palette(state, matches.len(), -1);
            Some(None)
        }
        KeyCode::Down => {
            move_palette(state, matches.len(), 1);
            Some(None)
        }
        KeyCode::Tab => {
            let selected = matches[state.palette_index.min(matches.len() - 1)];
            complete_from_palette(state, selected.name);
            Some(None)
        }
        KeyCode::Enter => {
            let selected_name = matches[state.palette_index.min(matches.len() - 1)].name;
            state.input.clear();
            state.palette_index = 0;
            Some(state.dispatch_command(selected_name))
        }
        _ => None,
    }
}

fn move_palette(state: &mut State, count: usize, delta: i32) {
    let len = count as i32;
    let next = (state.palette_index as i32 + delta).rem_euclid(len);
    state.palette_index = next as usize;
}

fn complete_from_palette(state: &mut State, name: &str) {
    state.input.clear();
    state.input.insert_str(&format!("/{name} "));
    state.palette_index = 0;
}

fn handle_picker_key(state: &mut State, key: KeyEvent) -> Option<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => Some(Effect::Quit),
        KeyCode::Up => {
            if let Some(picker) = state.picker.as_mut() {
                picker.move_by(-1);
            }
            None
        }
        KeyCode::Down => {
            if let Some(picker) = state.picker.as_mut() {
                picker.move_by(1);
            }
            None
        }
        KeyCode::Enter => state
            .picker
            .take()?
            .into_selected_id()
            .map(|id| Effect::Backend(BackendCommand::SwitchSession(id))),
        KeyCode::Esc => {
            state.picker = None;
            None
        }
        _ => None,
    }
}

fn handle_permission_key(state: &mut State, key: KeyEvent) -> Option<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let decision = match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Decision::Allow,
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            Decision::Deny("denied by user".into())
        }
        KeyCode::Char('c') if ctrl => return Some(Effect::Quit),
        _ => return None,
    };

    let pending = state.pending.take()?;
    Some(Effect::Backend(BackendCommand::PermissionDecision {
        id: pending.id,
        decision,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_on_unmatched_slash_command_dispatches_error() {
        let mut state = State::new("test-model".into());
        state.input.insert_str("/nope");

        let effect = update(
            &mut state,
            UiEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );

        assert!(effect.is_none());
        assert_eq!(state.input.as_str(), "");
    }
}
