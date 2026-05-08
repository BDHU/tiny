// Slash-command palette. An ambient overlay: visible iff input starts with
// `/` and matches at least one command. Navigation keys (Up/Down/Tab/Enter)
// are consumed; everything else passes through to default input handling.

use crate::tui::{
    commands::{self, Command},
    modal::{KeyDispatch, Modal, ModalOutcome, ModalSlot},
    state::AppState,
    surface::{choice_line, RenderCtx, Surface},
};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default)]
pub(crate) struct CommandPalette {
    selected: usize,
}

impl CommandPalette {
    fn matches_for(state: &AppState) -> Vec<&'static Command> {
        commands::palette_matches(state.input.as_str())
    }

    fn dispatch_selection(&mut self, state: &mut AppState) -> ModalOutcome {
        let matches = Self::matches_for(state);
        if matches.is_empty() {
            return ModalOutcome::Continue;
        }
        let name = matches[self.selected.min(matches.len() - 1)].name;
        state.input.clear();
        self.selected = 0;
        commands::dispatch_outcome(name, state.is_busy())
    }
}

impl Modal for CommandPalette {
    fn slot(&self) -> ModalSlot {
        ModalSlot::Panel
    }

    fn is_visible(&self, state: &AppState) -> bool {
        !Self::matches_for(state).is_empty()
    }

    fn render(&self, ctx: RenderCtx<'_>) -> Surface {
        if ctx.max_rows == 0 {
            return Surface::new();
        }
        let matches = Self::matches_for(ctx.state);
        if matches.is_empty() {
            return Surface::new();
        }
        let selected = self.selected.min(matches.len() - 1);
        let name_width = matches.iter().map(|c| c.name.len()).max().unwrap_or(0);
        let visible = matches.len().min(ctx.max_rows);
        let start = selected
            .saturating_sub(visible - 1)
            .min(matches.len() - visible);
        let mut surface = Surface::new();
        for offset in 0..visible {
            let i = start + offset;
            surface = surface.line(palette_row(matches[i], name_width, i == selected));
        }
        surface
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> KeyDispatch {
        let matches = Self::matches_for(state);
        if matches.is_empty() {
            // Reset selection so the next time the palette opens it starts
            // at the top instead of inheriting a stale index.
            self.selected = 0;
            return KeyDispatch::PassThrough;
        }
        match key.code {
            KeyCode::Up => {
                let len = matches.len() as i32;
                self.selected = (self.selected as i32 - 1).rem_euclid(len) as usize;
                KeyDispatch::Consumed(ModalOutcome::Continue)
            }
            KeyCode::Down => {
                let len = matches.len() as i32;
                self.selected = (self.selected as i32 + 1).rem_euclid(len) as usize;
                KeyDispatch::Consumed(ModalOutcome::Continue)
            }
            KeyCode::Tab => {
                let name = matches[self.selected.min(matches.len() - 1)].name;
                let replacement = format!("/{name} ");
                state.input.clear();
                state.input.insert_str(&replacement);
                self.selected = 0;
                KeyDispatch::Consumed(ModalOutcome::Continue)
            }
            KeyCode::Enter => {
                let outcome = self.dispatch_selection(state);
                KeyDispatch::Consumed(outcome)
            }
            _ => {
                // Editing keys (chars, backspace, esc) flow to the main
                // handler; reset selection so a brand-new match set starts
                // at the top rather than inheriting our stale index.
                self.selected = 0;
                KeyDispatch::PassThrough
            }
        }
    }
}

fn palette_row(cmd: &Command, name_width: usize, selected: bool) -> crate::tui::surface::Line {
    let pad = " ".repeat(name_width.saturating_sub(cmd.name.len()));
    let marker = if selected { " > " } else { "   " };
    let text = format!("{marker}/{}{}  {}", cmd.name, pad, cmd.help);
    choice_line(text, selected)
}
