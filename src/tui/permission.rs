use crate::backend::{Backend, BackendCommand, PermissionId};
use crate::tui::{
    modal::{Modal, ModalOutcome, ModalSlot},
    print,
    prompt::{fit_line, Frame},
    theme,
};
use crossterm::{
    event::{KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io;
use tiny::{Decision, ToolCall};

pub(crate) struct PermissionPromptModal {
    id: PermissionId,
    call: ToolCall,
}

impl PermissionPromptModal {
    pub(crate) fn new(id: PermissionId, call: ToolCall) -> Self {
        Self { id, call }
    }
}

impl Modal for PermissionPromptModal {
    fn slot(&self) -> ModalSlot {
        ModalSlot::Input
    }

    fn render(&self, frame: &mut Frame, term_cols: u16, _max_rows: usize) -> io::Result<()> {
        let text = format!(
            " allow {}({})?  y/n ",
            self.call.name,
            print::preview(&self.call.input.to_string(), 60)
        );
        queue!(
            frame,
            SetForegroundColor(theme::TOOL),
            SetAttribute(Attribute::Bold),
            Print(fit_line(&text, term_cols)),
            SetAttribute(Attribute::NormalIntensity),
            ResetColor,
        )
    }

    fn handle_key(&mut self, key: KeyEvent, backend: &Backend) -> ModalOutcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let decision = match key.code {
            KeyCode::Char('c') if ctrl => return ModalOutcome::Quit,
            KeyCode::Char('y') | KeyCode::Char('Y') => Decision::Allow,
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                Decision::Deny("denied by user".into())
            }
            _ => return ModalOutcome::Continue,
        };

        let _ = backend.commands.send(BackendCommand::PermissionDecision {
            id: self.id,
            decision,
        });
        ModalOutcome::Close
    }
}
