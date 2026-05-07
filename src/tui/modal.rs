// A Modal is a transient UI overlay that takes over either the input row
// (e.g. permission prompt) or the panel area below the input (e.g. session
// picker). Adding a new modal means writing one type that implements this
// trait — the prompt renderer and key dispatcher pick it up automatically.

use crate::backend::Backend;
use crate::tui::prompt::Frame;
use crossterm::event::KeyEvent;
use std::io;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalSlot {
    /// Replaces the input row. Cursor is parked at column 0 of that row.
    Input,
    /// Renders in the variable-height panel area below the input, above the
    /// status row. Cursor stays on the input.
    Panel,
}

pub(crate) enum ModalOutcome {
    /// Stay open.
    Continue,
    /// Close the modal.
    Close,
    /// Quit the app.
    Quit,
}

pub(crate) trait Modal {
    fn slot(&self) -> ModalSlot;

    /// Append rows to `frame` for this modal. `max_rows` is the budget for
    /// `Panel` modals (variable area between input and status); `Input`
    /// modals always render exactly one row.
    fn render(&self, frame: &mut Frame, term_cols: u16, max_rows: usize) -> io::Result<()>;

    fn handle_key(&mut self, key: KeyEvent, backend: &Backend) -> ModalOutcome;
}
