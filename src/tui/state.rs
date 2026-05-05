use crate::{
    backend::PermissionId,
    tui::transcript::{Entry, Transcript},
};
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
            input: String::new(),
            cursor: 0,
            transcript: Transcript::default(),
            scroll: ScrollState {
                follow_tail: true,
                ..ScrollState::default()
            },
            tick: 0,
            model,
            turn: TurnState {
                busy: false,
                started_at: None,
            },
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
        let max_scroll = self.max_scroll();
        self.scroll.offset = self.scroll.offset.min(max_scroll);
        if self.scroll.offset == max_scroll {
            self.scroll.follow_tail = true;
        }
    }

    fn scroll_lines(&mut self, lines: i16) {
        if lines < 0 {
            self.scroll.offset = self.scroll.offset.saturating_sub(lines.unsigned_abs());
            self.scroll.follow_tail = false;
        } else {
            self.scroll.offset = self.scroll.offset.saturating_add(lines as u16);
            self.clamp_scroll();
        }
    }
}

pub(crate) fn update(state: &mut State, event: UiEvent) -> Vec<Effect> {
    match event {
        UiEvent::Key(key) => handle_key(state, key),
        UiEvent::Paste(text) => {
            if state.pending.is_none() {
                state.insert_str(&text);
            }
            Vec::new()
        }
        UiEvent::Scroll(lines) => {
            state.scroll_lines(lines);
            Vec::new()
        }
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
            Vec::new()
        }
        UiEvent::Entry(entry) => {
            state.transcript.push(entry);
            if state.scroll.follow_tail {
                state.scroll_to_bottom();
            }
            Vec::new()
        }
        UiEvent::PermissionRequest { id, call } => {
            state.pending = Some(PendingPermission { id, call });
            Vec::new()
        }
        UiEvent::TurnStarted => {
            if !state.turn.busy && state.queued > 0 {
                state.queued -= 1;
            }
            state.turn.busy = true;
            state.turn.started_at = Some(Instant::now());
            Vec::new()
        }
        UiEvent::TurnError(error) => {
            state.transcript.push(Entry::Error(error));
            Vec::new()
        }
        UiEvent::TurnDone => {
            state.turn.busy = false;
            state.turn.started_at = None;
            Vec::new()
        }
        UiEvent::Tick => {
            state.tick = state.tick.wrapping_add(1);
            Vec::new()
        }
    }
}

fn handle_key(state: &mut State, key: KeyEvent) -> Vec<Effect> {
    if state.pending.is_some() {
        return handle_permission_key(state, key);
    }

    match key.code {
        KeyCode::Char('d') | KeyCode::Char('c')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            vec![Effect::Quit]
        }
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vec![Effect::Redraw]
        }
        KeyCode::Enter if !state.input.trim().is_empty() => {
            let input = state.clear_input();
            state.transcript.push(Entry::User(input.clone()));
            state.scroll.follow_tail = true;
            if state.turn.busy {
                state.queued += 1;
            } else {
                state.turn.busy = true;
                state.turn.started_at = Some(Instant::now());
            }
            vec![Effect::Submit(input)]
        }
        KeyCode::Char(c) => {
            state.insert_char(c);
            Vec::new()
        }
        KeyCode::Backspace => {
            state.backspace();
            Vec::new()
        }
        KeyCode::Left => {
            state.move_left();
            Vec::new()
        }
        KeyCode::Right => {
            state.move_right();
            Vec::new()
        }
        KeyCode::Esc => {
            state.clear_input();
            Vec::new()
        }
        KeyCode::PageUp => {
            state.scroll.offset = state.scroll.offset.saturating_sub(10);
            state.scroll.follow_tail = false;
            Vec::new()
        }
        KeyCode::PageDown => {
            state.scroll.offset = state.scroll.offset.saturating_add(10);
            state.clamp_scroll();
            Vec::new()
        }
        KeyCode::Up => {
            state.scroll.offset = state.scroll.offset.saturating_sub(1);
            state.scroll.follow_tail = false;
            Vec::new()
        }
        KeyCode::Down => {
            state.scroll.offset = state.scroll.offset.saturating_add(1);
            state.clamp_scroll();
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn handle_permission_key(state: &mut State, key: KeyEvent) -> Vec<Effect> {
    let decision = match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(Decision::Allow),
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            Some(Decision::Deny("denied by user".into()))
        }
        KeyCode::Char('c') | KeyCode::Char('d')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            return vec![Effect::Quit];
        }
        _ => None,
    };

    match (state.pending.take(), decision) {
        (Some(pending), Some(decision)) => vec![Effect::ReplyPermission {
            id: pending.id,
            decision,
        }],
        (pending, None) => {
            state.pending = pending;
            Vec::new()
        }
        (None, _) => Vec::new(),
    }
}
