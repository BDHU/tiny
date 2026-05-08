use crate::tui::{prompt::Prompt, state::AppState};
use anyhow::Result;
use crossterm::terminal;
use std::io::Write;

pub(crate) fn render_screen<W: Write>(
    out: &mut W,
    prompt: &mut Prompt,
    state: &AppState,
) -> Result<()> {
    let term_size = terminal::size().unwrap_or((80, 24));
    prompt.render(out, state, term_size)?;
    Ok(())
}
