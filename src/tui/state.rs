use crate::backend::PermissionId;
use crate::tui::transcript::{Entry, Transcript};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;
use tiny::{Decision, ToolCall};

pub(crate) struct PendingPermission {
    pub(crate) id: PermissionId,
    pub(crate) call: ToolCall,
}

#[derive(Default)]
pub(crate) struct ScrollState {
    pub(crate) offset: u16,
    pub(crate) follow_tail: bool,
    viewport_height: u16,
    content_height: u16,
}

#[derive(Default)]
pub(crate) struct TurnState {
    pub(crate) busy: bool,
    pub(crate) started_at: Option<Instant>,
}

pub(crate) struct State {
    pub(crate) input: String,
    cursor: usize,
    pub(crate) transcript: Transcript,
    pub(crate) scroll: ScrollState,
    pub(crate) tick: usize,
    pub(crate) model: String,
    pub(crate) turn: TurnState,
    pub(crate) queued: usize,
    pub(crate) pending: Option<PendingPermission>,
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
            input: String::new(),
            cursor: 0,
            transcript: Transcript::default(),
            scroll: ScrollState {
                follow_tail: true,
                ..ScrollState::default()
            },
            tick: 0,
            model,
            turn: TurnState::default(),
            queued: 0,
            pending: None,
        }
    }

    pub(crate) fn cursor_column(&self) -> u16 {
        self.input[..self.cursor].chars().count() as u16
    }

    fn clear_input(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.input)
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn insert_str(&mut self, s: &str) {
        let cleaned: String = s
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        self.input.insert_str(self.cursor, &cleaned);
        self.cursor += cleaned.len();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.move_left();
        self.input.remove(self.cursor);
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        while !self.input.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        self.cursor += 1;
        while !self.input.is_char_boundary(self.cursor) {
            self.cursor += 1;
        }
    }

    fn max_scroll(&self) -> u16 {
        self.scroll
            .content_height
            .saturating_sub(self.scroll.viewport_height)
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll.offset = self.max_scroll();
        self.scroll.follow_tail = true;
    }

    fn clamp_scroll(&mut self) {
        let max = self.max_scroll();
        self.scroll.offset = self.scroll.offset.min(max);
        if self.scroll.offset == max {
            self.scroll.follow_tail = true;
        }
    }

    fn scroll_by(&mut self, delta: i16) {
        if delta < 0 {
            self.scroll.offset = self.scroll.offset.saturating_sub(delta.unsigned_abs());
            self.scroll.follow_tail = false;
        } else {
            self.scroll.offset = self.scroll.offset.saturating_add(delta as u16);
            self.clamp_scroll();
        }
    }

    fn begin_turn(&mut self) {
        self.turn.busy = true;
        self.turn.started_at = Some(Instant::now());
    }
}

pub(crate) fn update(state: &mut State, event: UiEvent) -> Option<Effect> {
    match event {
        UiEvent::Key(key) => return handle_key(state, key),
        UiEvent::Paste(text) => {
            if state.pending.is_none() {
                state.insert_str(&text);
            }
        }
        UiEvent::Scroll(lines) => state.scroll_by(lines),
        UiEvent::Viewport { width, height } => {
            state.transcript.resize(width);
            state.scroll.viewport_height = height;
            state.scroll.content_height = state.transcript.height();
            if state.turn.busy {
                state.scroll.content_height = state.scroll.content_height.saturating_add(2);
            }
            if state.scroll.follow_tail {
                state.scroll_to_bottom();
            } else {
                state.clamp_scroll();
            }
        }
        UiEvent::Entry(entry) => {
            state.transcript.push(entry);
            if state.scroll.follow_tail {
                state.scroll_to_bottom();
            }
        }
        UiEvent::PermissionRequest { id, call } => {
            state.pending = Some(PendingPermission { id, call });
        }
        UiEvent::TurnStarted => {
            if !state.turn.busy && state.queued > 0 {
                state.queued -= 1;
            }
            state.begin_turn();
        }
        UiEvent::TurnError(error) => state.transcript.push(Entry::Error(error)),
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
        KeyCode::Enter if !state.input.trim().is_empty() => {
            let input = state.clear_input();
            state.transcript.push(Entry::User(input.clone()));
            state.scroll.follow_tail = true;
            if state.turn.busy {
                state.queued += 1;
            } else {
                state.begin_turn();
            }
            Some(Effect::Submit(input))
        }
        KeyCode::Char(c) => {
            state.insert_char(c);
            None
        }
        KeyCode::Backspace => {
            state.backspace();
            None
        }
        KeyCode::Left => {
            state.move_left();
            None
        }
        KeyCode::Right => {
            state.move_right();
            None
        }
        KeyCode::Esc => {
            state.clear_input();
            None
        }
        KeyCode::PageUp => {
            state.scroll_by(-10);
            None
        }
        KeyCode::PageDown => {
            state.scroll_by(10);
            None
        }
        KeyCode::Up => {
            state.scroll_by(-1);
            None
        }
        KeyCode::Down => {
            state.scroll_by(1);
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
