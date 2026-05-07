use crate::backend::{Backend, BackendCommand};
use crate::tui::{
    modal::{Modal, ModalOutcome, ModalSlot},
    prompt::{fit_line, write_choice_line, Frame},
    theme,
};
use crossterm::{
    event::{KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Print, ResetColor, SetForegroundColor},
};
use std::io;
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

    pub(crate) fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    fn move_by(&mut self, delta: i32) {
        let len = self.sessions.len() as i32;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(len) as usize;
    }

    fn selected_id(&self) -> Option<SessionId> {
        let idx = self.selected.min(self.sessions.len().checked_sub(1)?);
        self.sessions.get(idx).map(|m| m.id.clone())
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
    fn slot(&self) -> ModalSlot {
        ModalSlot::Panel
    }

    fn render(&self, frame: &mut Frame, term_cols: u16, max_rows: usize) -> io::Result<()> {
        if self.is_empty() || max_rows < 2 {
            return Ok(());
        }
        let items_budget = (max_rows - 1).min(20);
        let selected_index = self.selected.min(self.sessions.len() - 1);

        frame.row()?;
        queue!(
            frame,
            SetForegroundColor(theme::DIM),
            Print(fit_line(
                " sessions · enter resume · esc cancel ",
                term_cols
            )),
            ResetColor,
        )?;

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

            frame.row()?;
            write_choice_line(frame, &text, is_selected, term_cols)?;
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent, backend: &Backend) -> ModalOutcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => ModalOutcome::Quit,
            KeyCode::Up => {
                self.move_by(-1);
                ModalOutcome::Continue
            }
            KeyCode::Down => {
                self.move_by(1);
                ModalOutcome::Continue
            }
            KeyCode::Enter => {
                if let Some(id) = self.selected_id() {
                    let _ = backend.commands.send(BackendCommand::SwitchSession(id));
                }
                ModalOutcome::Close
            }
            KeyCode::Esc => ModalOutcome::Close,
            _ => ModalOutcome::Continue,
        }
    }
}
