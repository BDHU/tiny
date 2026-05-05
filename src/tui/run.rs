use crate::tui::app::{entries_from_message, handle_key, Action, App, Entry, PendingPermission};
use crate::tui::line::line_mode;
use crate::tui::render::ui;
use anyhow::Result;
use crossterm::{
    event::{Event as CtEvent, EventStream, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io::{stdin, stdout, IsTerminal},
    time::Duration,
};
use tiny::{Agent, Event};
use tokio::sync::mpsc;

enum StateEvent {
    Error(String),
    Done,
}

struct TerminalSession(Terminal<CrosstermBackend<std::io::Stdout>>);

impl TerminalSession {
    fn enter() -> Result<Self> {
        if !has_interactive_terminal() {
            anyhow::bail!("the TUI must be run in an interactive terminal");
        }

        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        Ok(Self(Terminal::new(CrosstermBackend::new(stdout()))?))
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.0.backend_mut(), LeaveAlternateScreen);
    }
}

fn has_interactive_terminal() -> bool {
    stdin().is_terminal() && stdout().is_terminal()
}

async fn drive_turns(
    mut agent: Agent,
    mut inputs: mpsc::Receiver<String>,
    agent_events: mpsc::UnboundedSender<Event>,
    state_events: mpsc::UnboundedSender<StateEvent>,
) {
    while let Some(input) = inputs.recv().await {
        let result = agent.send(input, &agent_events).await;

        if let Err(e) = result {
            let _ = state_events.send(StateEvent::Error(e.to_string()));
        }
        let _ = state_events.send(StateEvent::Done);
    }
}

fn handle_agent_event(app: &mut App, event: Event) {
    match event {
        Event::Message(message) => {
            for entry in entries_from_message(&message) {
                app.push_entry(entry);
            }
        }
        Event::PermissionRequest { call, reply } => {
            app.pending = Some(PendingPermission { call, reply });
        }
    }
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    inputs: mpsc::Sender<String>,
    mut agent_events: mpsc::UnboundedReceiver<Event>,
    mut state_events: mpsc::UnboundedReceiver<StateEvent>,
) -> Result<()> {
    let mut keys = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(80));

    loop {
        if app.auto_scroll {
            let height = terminal.size()?.height.saturating_sub(4);
            app.scroll_to_bottom(height);
        }

        terminal.draw(|f| ui(f, app))?;

        tokio::select! {
            _ = ticker.tick() => {
                app.tick = app.tick.wrapping_add(1);
            }

            Some(event) = agent_events.recv() => {
                handle_agent_event(app, event);
            }

            Some(event) = state_events.recv() => match event {
                StateEvent::Error(error) => app.push_entry(Entry::Error(error)),
                StateEvent::Done => app.waiting = false,
            },

            Some(Ok(key_event)) = keys.next() => {
                if let CtEvent::Key(key) = key_event {
                    if key.kind != KeyEventKind::Press { continue; }

                    match handle_key(app, key) {
                        Some(Action::Quit) => break,
                        Some(Action::Submit(input)) => {
                            app.waiting = true;
                            let _ = inputs.send(input).await;
                        }
                        None => {}
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn run(agent: Agent, model: String) -> Result<()> {
    if !has_interactive_terminal() {
        return line_mode(agent, model).await;
    }

    let mut app = App::new(model);
    let mut terminal = TerminalSession::enter()?;

    let (input_tx, input_rx) = mpsc::channel(1);
    let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel();
    let (state_event_tx, state_event_rx) = mpsc::unbounded_channel();

    tokio::spawn(drive_turns(agent, input_rx, agent_event_tx, state_event_tx));

    event_loop(
        &mut terminal.0,
        &mut app,
        input_tx,
        agent_event_rx,
        state_event_rx,
    )
    .await
}
