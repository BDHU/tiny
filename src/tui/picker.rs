use tiny::{SessionId, SessionMeta};

pub(crate) struct SessionPicker {
    pub(crate) sessions: Vec<SessionMeta>,
    pub(crate) selected: usize,
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

    pub(crate) fn into_selected_id(self) -> Option<SessionId> {
        let selected = self.selected.min(self.sessions.len().checked_sub(1)?);
        self.sessions.into_iter().nth(selected).map(|meta| meta.id)
    }
}
