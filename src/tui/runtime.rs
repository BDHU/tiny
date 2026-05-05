use crate::{
    backend::{self, Backend, BackendCommand},
    tui::{
        events, reader,
        state::{self, Effect, State, UiEvent},
        view,
    },
};
use anyhow::Result;
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};
use std::time::Duration;
use tiny::Agent;
use tokio::sync::mpsc;

type Term = Terminal<CrosstermBackend<std::io::Stdout>>;

pub(crate) async fn run(terminal: &mut Term, agent: Agent, model: String) -> Result<()> {
    let mut state = State::new(model);
    let mut backend = backend::spawn(agent);
    let (reader_tx, mut reader_rx) = mpsc::unbounded_channel();
    let _reader = reader::spawn(reader_tx);
    let mut ticker = tokio::time::interval(Duration::from_millis(80));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    draw(terminal, &mut state)?;
    loop {
        let (events, redraw) = tokio::select! {
            _ = ticker.tick() => {
                if state.turn.busy {
                    (vec![UiEvent::Tick], true)
                } else {
                    (Vec::new(), false)
                }
            }
            Some(event) = backend.events.recv() => {
                (drain(event, &mut backend.events, events::from_backend), true)
            }
            Some(event) = reader_rx.recv() => {
                (drain(event, &mut reader_rx, events::from_reader), true)
            }
            else => break,
        };

        if !drive(terminal, &backend, &mut state, events)? {
            break;
        }
        if redraw {
            draw(terminal, &mut state)?;
        }
    }

    Ok(())
}

fn drain<T>(
    first: T,
    rx: &mut mpsc::UnboundedReceiver<T>,
    map: impl Fn(T) -> Vec<UiEvent>,
) -> Vec<UiEvent> {
    let mut out = map(first);
    while let Ok(event) = rx.try_recv() {
        out.extend(map(event));
    }
    out
}

fn drive(
    terminal: &mut Term,
    backend: &Backend,
    state: &mut State,
    events: Vec<UiEvent>,
) -> Result<bool> {
    for event in events {
        if let Some(effect) = state::update(state, event) {
            if !apply(terminal, backend, effect)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn apply(terminal: &mut Term, backend: &Backend, effect: Effect) -> Result<bool> {
    match effect {
        Effect::Quit => Ok(false),
        Effect::Submit(input) => {
            let _ = backend.commands.send(BackendCommand::Submit(input));
            Ok(true)
        }
        Effect::ReplyPermission { id, decision } => {
            let _ = backend
                .commands
                .send(BackendCommand::PermissionDecision { id, decision });
            Ok(true)
        }
        Effect::Redraw => {
            terminal.clear()?;
            Ok(true)
        }
    }
}

fn draw(terminal: &mut Term, state: &mut State) -> Result<()> {
    let size = terminal.size()?;
    let area = view::message_area(Rect::new(0, 0, size.width, size.height));
    state::update(
        state,
        UiEvent::Viewport {
            width: area.width,
            height: area.height,
        },
    );
    terminal.draw(|f| view::ui(f, state))?;
    Ok(())
}
