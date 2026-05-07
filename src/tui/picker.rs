use std::ops::Range;
use tiny::{SessionId, SessionMeta};

pub(crate) struct SessionPicker {
    sessions: Vec<SessionMeta>,
    selected: usize,
}

impl SessionPicker {
    pub(crate) fn new(sessions: Vec<SessionMeta>) -> Self {
        Self {
            sessions,
            selected: 0,
        }
    }

    pub(crate) fn move_by(&mut self, delta: i32) {
        let len = self.sessions.len() as i32;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(len) as usize;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        (!self.sessions.is_empty()).then_some(self.selected.min(self.sessions.len() - 1))
    }

    pub(crate) fn session(&self, index: usize) -> Option<&SessionMeta> {
        self.sessions.get(index)
    }

    pub(crate) fn visible_range(&self, max_rows: usize) -> Range<usize> {
        let total = self.sessions.len();
        if total == 0 {
            return 0..0;
        }

        let window = max_rows.min(total).max(1);
        let selected = self.selected.min(total - 1);
        let start = selected.saturating_add(1).saturating_sub(window);
        start..(start + window).min(total)
    }

    pub(crate) fn into_selected_id(self) -> Option<SessionId> {
        let selected = self.selected.min(self.sessions.len().checked_sub(1)?);
        self.sessions.into_iter().nth(selected).map(|meta| meta.id)
    }
}
