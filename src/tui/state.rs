use crate::backend::PermissionId;
use crate::tui::input::InputBuffer;
use crate::tui::scroll::ScrollState;
use crate::tui::transcript::{Entry, Transcript};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;
use tiny::{Decision, ToolCall};

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
}

pub(crate) enum UiEvent {
    Key(KeyEvent),
    Paste(String),
    Scroll(i16),
    Viewport { width: u16, height: u16 },
    Entry(Entry),
    PermissionRequest { id: PermissionId, call: ToolCall },
    TurnStarted,
    TurnError(String),
    TurnDone,
    Tick,
}

pub(crate) enum Effect {
    Quit,
    Submit(String),
    ReplyPermission {
        id: PermissionId,
        decision: Decision,
    },
    Redraw,
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

    fn submit_input(&mut self) -> Effect {
        let input = self.input.clear();
        self.push_entry(Entry::User(input.clone()));
        self.scroll.follow_tail();
        self.begin_or_queue_turn();
        Effect::Submit(input)
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
    }
    None
}

fn handle_key(state: &mut State, key: KeyEvent) -> Option<Effect> {
    if state.pending.is_some() {
        return handle_permission_key(state, key);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('d') | KeyCode::Char('c') if ctrl => Some(Effect::Quit),
        KeyCode::Char('l') if ctrl => Some(Effect::Redraw),
        KeyCode::Enter if !state.input.is_blank() => Some(state.submit_input()),
        KeyCode::Char(c) => {
            state.input.insert_char(c);
            None
        }
        KeyCode::Backspace => {
            state.input.backspace();
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

fn handle_permission_key(state: &mut State, key: KeyEvent) -> Option<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let decision = match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Decision::Allow,
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            Decision::Deny("denied by user".into())
        }
        KeyCode::Char('c') | KeyCode::Char('d') if ctrl => return Some(Effect::Quit),
        _ => return None,
    };

    let pending = state.pending.take()?;
    Some(Effect::ReplyPermission {
        id: pending.id,
        decision,
    })
}
