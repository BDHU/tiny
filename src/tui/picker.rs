use crate::backend::BackendCommand;
use crate::tui::{
    modal::{KeyDispatch, Modal, ModalOutcome},
    state::AppState,
    surface::{choice_line, Line, RenderCtx, Style, Surface},
    theme,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::ops::Range;
use tiny::{SessionId, SessionMeta};

pub(crate) struct SessionPicker {
    sessions: Vec<SessionMeta>,
    selected: usize,
    active_session_id: Option<String>,
}

impl SessionPicker {
    pub(crate) fn new(sessions: Vec<SessionMeta>, active_session_id: Option<String>) -> Self {
        Self {
            sessions,
            selected: 0,
            active_session_id,
        }
    }

    fn move_by(&mut self, delta: i32) {
        let len = self.sessions.len() as i32;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(len) as usize;
    }

    fn selected_id(&self) -> Option<SessionId> {
        self.sessions.get(self.selected).map(|m| m.id.clone())
    }

    fn visible_range(&self, max_rows: usize) -> Range<usize> {
        let total = self.sessions.len();
        if total == 0 {
            return 0..0;
        }
        let window = max_rows.min(total).max(1);
        let selected = self.selected.min(total - 1);
        let start = selected.saturating_add(1).saturating_sub(window);
        start..(start + window).min(total)
    }
}

impl Modal for SessionPicker {
    fn render(&self, ctx: RenderCtx<'_>) -> Surface {
        if self.sessions.is_empty() || ctx.max_rows < 2 {
            return Surface::new();
        }
        let items_budget = (ctx.max_rows - 1).min(20);
        let selected_index = self.selected;
        let mut surface = Surface::new().line(Line::styled(
            " sessions · enter resume · esc cancel ",
            Style::fg(theme::DIM),
        ));

        for i in self.visible_range(items_budget) {
            let Some(meta) = self.sessions.get(i) else {
                continue;
            };
            let is_selected = i == selected_index;
            let is_active = self.active_session_id.as_deref() == Some(meta.id.0.as_str());
            let marker = if is_selected { ">" } else { " " };
            let active_marker = if is_active { "*" } else { " " };
            let title = if meta.title.is_empty() {
                "(untitled)"
            } else {
                meta.title.as_str()
            };
            let text = format!(" {marker}{active_marker} {title}");
            surface.push_line(choice_line(text, is_selected));
        }
        surface
    }

    fn handle_key(&mut self, key: KeyEvent, _state: &mut AppState) -> KeyDispatch {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => KeyDispatch::Consumed(ModalOutcome::Quit),
            KeyCode::Up => {
                self.move_by(-1);
                KeyDispatch::Consumed(ModalOutcome::Continue)
            }
            KeyCode::Down => {
                self.move_by(1);
                KeyDispatch::Consumed(ModalOutcome::Continue)
            }
            KeyCode::Enter => match self.selected_id() {
                Some(id) => KeyDispatch::Consumed(ModalOutcome::EmitAndClose(
                    BackendCommand::SwitchSession(id),
                )),
                None => KeyDispatch::Consumed(ModalOutcome::Close),
            },
            KeyCode::Esc => KeyDispatch::Consumed(ModalOutcome::Close),
            _ => KeyDispatch::Consumed(ModalOutcome::Continue),
        }
    }
}
