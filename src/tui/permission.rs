use crate::backend::{BackendCommand, PermissionId};
use crate::tui::{
    modal::{KeyDispatch, Modal, ModalOutcome},
    print,
    state::AppState,
    surface::{Line, RenderCtx, Style, Surface},
    theme,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
    fn render(&self, ctx: RenderCtx<'_>) -> Surface {
        if ctx.max_rows == 0 {
            return Surface::new();
        }
        let preview_limit = usize::from(ctx.cols.saturating_sub(20)).clamp(20, 60);
        let text = format!(
            " allow {}({})?  y/n ",
            self.call.name,
            print::preview(&self.call.input.to_string(), preview_limit)
        );
        Surface::new().line(Line::styled(text, Style::fg(theme::TOOL).bold()))
    }

    fn handle_key(&mut self, key: KeyEvent, _state: &mut AppState) -> KeyDispatch {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let decision = match key.code {
            KeyCode::Char('c') if ctrl => return KeyDispatch::Consumed(ModalOutcome::Quit),
            KeyCode::Char('y') | KeyCode::Char('Y') => Decision::Allow,
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                Decision::Deny("denied by user".into())
            }
            _ => return KeyDispatch::Consumed(ModalOutcome::Continue),
        };
        KeyDispatch::Consumed(ModalOutcome::EmitAndClose(
            BackendCommand::PermissionDecision {
                id: self.id,
                decision,
            },
        ))
    }
}
