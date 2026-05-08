// A Modal is a UI overlay that participates in rendering and key dispatch.
// There are two flavors:
//
//   * Takeover modals (`state.modal`): when set, every keystroke is routed to
//     the modal. Used by the permission prompt and session picker.
//   * Ambient overlays (`state.overlay`): permanently installed; consume
//     navigation keys but pass everything else through to default handling.
//     Currently houses the slash-command palette.
//
// Both go through this trait; the dispatcher in `keys.rs` decides routing.

use crate::backend::BackendCommand;
use crate::tui::print::Entry;
use crate::tui::state::AppState;
use crate::tui::surface::{RenderCtx, Surface};
use crossterm::event::KeyEvent;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalSlot {
    /// Replaces the input row. Cursor is parked at column 0 of that row.
    Input,
    /// Renders in the variable-height panel area below the input, above the
    /// status row. Cursor stays on the input.
    Panel,
}

pub(crate) enum ModalOutcome {
    /// Stay open; no side effect.
    Continue,
    /// Close the takeover modal (no-op for overlays — they always reinstate).
    Close,
    /// Quit the app.
    Quit,
    /// Forward a backend command and stay open.
    Emit(BackendCommand),
    /// Forward a backend command and close the takeover modal.
    EmitAndClose(BackendCommand),
    /// Print to scrollback and stay open.
    Print(Entry),
}

pub(crate) enum KeyDispatch {
    Consumed(ModalOutcome),
    /// The modal didn't claim this key — fall through to default handling.
    /// Only meaningful for overlays; takeover modals should consume everything.
    PassThrough,
}

pub(crate) trait Modal {
    fn slot(&self) -> ModalSlot;

    /// Whether to render this frame. Default: always. Overlays override to
    /// hide themselves when their trigger isn't satisfied (e.g. palette
    /// hides when input doesn't start with `/`).
    fn is_visible(&self, _state: &AppState) -> bool {
        true
    }

    fn render(&self, ctx: RenderCtx<'_>) -> Surface;

    /// While this runs, the modal has been taken out of its slot — `state.modal`
    /// (or `state.overlay`) is `None`. The modal is free to mutate other
    /// fields like `state.input`.
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> KeyDispatch;
}
