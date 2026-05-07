use crate::tui::runtime;
use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::io::{stdin, stdout, IsTerminal, Write};
use std::sync::Arc;
use tiny::AgentConfig;

struct RawSession;

impl RawSession {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), EnableBracketedPaste)?;
        Ok(Self)
    }
}

impl Drop for RawSession {
    fn drop(&mut self) {
        let _ = execute!(stdout(), DisableBracketedPaste);
        let _ = disable_raw_mode();
        // Leave the cursor on a fresh line so the shell prompt comes back cleanly.
        let mut out = stdout();
        let _ = out.write_all(b"\r\n");
        let _ = out.flush();
    }
}

pub async fn run(config: Arc<AgentConfig>, model: String) -> Result<()> {
    if !stdin().is_terminal() || !stdout().is_terminal() {
        anyhow::bail!("the TUI must be run in an interactive terminal");
    }

    let _session = RawSession::enter()?;
    let mut out = stdout();
    runtime::run(&mut out, config, model).await
}
