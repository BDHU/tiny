use crate::tui::runtime;
use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{stdin, stdout, IsTerminal};
use std::sync::Arc;
use tiny::AgentConfig;

struct TerminalSession(Terminal<CrosstermBackend<std::io::Stdout>>);

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        Ok(Self(Terminal::new(CrosstermBackend::new(stdout()))?))
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(
            self.0.backend_mut(),
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

pub async fn run(config: Arc<AgentConfig>, model: String) -> Result<()> {
    if !stdin().is_terminal() || !stdout().is_terminal() {
        anyhow::bail!("the TUI must be run in an interactive terminal");
    }

    let mut terminal = TerminalSession::enter()?;
    runtime::run(&mut terminal.0, config, model).await
}
