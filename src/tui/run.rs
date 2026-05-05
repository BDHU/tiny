use crate::tui::{line::line_mode, runtime};
use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{stdin, stdout, IsTerminal};
use tiny::Agent;

struct TerminalSession(Terminal<CrosstermBackend<std::io::Stdout>>);

impl TerminalSession {
    fn enter() -> Result<Self> {
        if !has_interactive_terminal() {
            anyhow::bail!("the TUI must be run in an interactive terminal");
        }

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

fn has_interactive_terminal() -> bool {
    stdin().is_terminal() && stdout().is_terminal()
}

pub async fn run(agent: Agent, model: String) -> Result<()> {
    if !has_interactive_terminal() {
        return line_mode(agent, model).await;
    }

    let mut terminal = TerminalSession::enter()?;
    runtime::run(&mut terminal.0, agent, model).await
}
