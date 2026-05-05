use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
use tiny::{Decision, Message, ToolCall};
use tokio::sync::oneshot;

pub(crate) enum Entry {
    User(String),
    Assistant(String),
    ToolCall { name: String, args: Value },
    ToolResult { content: String, is_error: bool },
    Error(String),
}

pub(crate) enum Action {
    Quit,
    Submit(String),
}

pub(crate) struct PendingPermission {
    pub(crate) call: ToolCall,
    pub(crate) reply: oneshot::Sender<Decision>,
}

pub(crate) struct App {
    pub(crate) input: String,
    cursor: usize,
    pub(crate) entries: Vec<Entry>,
    pub(crate) scroll: u16,
    pub(crate) auto_scroll: bool,
    pub(crate) tick: usize,
    pub(crate) model: String,
    pub(crate) waiting: bool,
    pub(crate) pending: Option<PendingPermission>,
}

impl Entry {
    fn height(&self) -> u16 {
        match self {
            Entry::User(_) | Entry::Error(_) => 2,
            Entry::Assistant(text) => text.lines().count() as u16 + 1,
            Entry::ToolCall { .. } | Entry::ToolResult { .. } => 1,
        }
    }
}

pub(crate) fn entries_from_message(message: &Message) -> Vec<Entry> {
    match message {
        Message::User(text) => vec![Entry::User(text.clone())],
        Message::Assistant { text, tool_calls } => {
            let mut entries = Vec::new();
            if !text.is_empty() {
                entries.push(Entry::Assistant(text.clone()));
            }
            entries.extend(tool_calls.iter().map(|call| Entry::ToolCall {
                name: call.name.clone(),
                args: call.input.clone(),
            }));
            entries
        }
        Message::Tool(result) => vec![Entry::ToolResult {
            content: result.content.clone(),
            is_error: result.is_error,
        }],
    }
}

impl App {
    pub(crate) fn new(model: String) -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            entries: Vec::new(),
            scroll: 0,
            auto_scroll: false,
            tick: 0,
            model,
            waiting: false,
            pending: None,
        }
    }

    pub(crate) fn scroll_to_bottom(&mut self, viewport_height: u16) {
        let content_height = self.entries.iter().map(Entry::height).sum::<u16>();
        self.scroll = content_height.saturating_sub(viewport_height);
        self.auto_scroll = false;
    }

    pub(crate) fn push_entry(&mut self, entry: Entry) {
        self.entries.push(entry);
        self.auto_scroll = true;
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
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) -> Option<Action> {
    if app.pending.is_some() {
        let decision = match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(Decision::Allow),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                Some(Decision::Deny("denied by user".into()))
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(Action::Quit);
            }
            _ => None,
        };
        if let Some(decision) = decision {
            if let Some(p) = app.pending.take() {
                let _ = p.reply.send(decision);
            }
        }
        return None;
    }

    match key.code {
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Enter if !app.waiting && !app.input.trim().is_empty() => {
            Some(Action::Submit(app.clear_input()))
        }
        KeyCode::Char(c) if !app.waiting => {
            app.insert_char(c);
            None
        }
        KeyCode::Backspace if !app.waiting => {
            app.backspace();
            None
        }
        KeyCode::Left => {
            app.move_left();
            None
        }
        KeyCode::Right => {
            app.move_right();
            None
        }
        KeyCode::Esc => {
            app.clear_input();
            None
        }
        KeyCode::PageUp => {
            app.scroll = app.scroll.saturating_sub(10);
            None
        }
        KeyCode::PageDown => {
            app.scroll = app.scroll.saturating_add(10);
            None
        }
        KeyCode::Up => {
            app.scroll = app.scroll.saturating_sub(1);
            None
        }
        KeyCode::Down => {
            app.scroll = app.scroll.saturating_add(1);
            None
        }
        _ => None,
    }
}
