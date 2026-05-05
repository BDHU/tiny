use crate::{
    backend::{self, Backend, BackendCommand, BackendEvent},
    tui::{
        reader::{self, ReaderEvent},
        state::{self, Effect, State, UiEvent},
        transcript::Entry,
        view,
    },
};
use anyhow::Result;
use crossterm::event::{Event as CtEvent, KeyEventKind, MouseEventKind};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};
use std::time::Duration;
use tiny::{Agent, Message};
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
                let mut out = from_backend(event);
                while let Ok(more) = backend.events.try_recv() {
                    out.extend(from_backend(more));
                }
                (out, true)
            }
            Some(event) = reader_rx.recv() => {
                let mut out = from_reader(event);
                while let Ok(more) = reader_rx.try_recv() {
                    out.extend(from_reader(more));
                }
                (out, true)
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

fn from_backend(event: BackendEvent) -> Vec<UiEvent> {
    match event {
        // The agent records the user message as the first event of every turn —
        // we already showed it locally on Enter, so suppress the duplicate.
        BackendEvent::Message(Message::User(_)) => Vec::new(),
        BackendEvent::Message(Message::Assistant { text, tool_calls }) => {
            let mut out = Vec::new();
            if !text.is_empty() {
                out.push(UiEvent::Entry(Entry::Assistant(text)));
            }
            out.extend(tool_calls.into_iter().map(|c| {
                UiEvent::Entry(Entry::ToolCall {
                    name: c.name,
                    args: c.input,
                })
            }));
            out
        }
        BackendEvent::Message(Message::Tool(result)) => vec![UiEvent::Entry(Entry::ToolResult {
            content: result.content,
            is_error: result.is_error,
        })],
        BackendEvent::PermissionRequest { id, call } => {
            vec![UiEvent::PermissionRequest { id, call }]
        }
        BackendEvent::TurnStarted => vec![UiEvent::TurnStarted],
        BackendEvent::TurnError(error) => vec![UiEvent::TurnError(error)],
        BackendEvent::TurnDone => vec![UiEvent::TurnDone],
    }
}

fn from_reader(event: ReaderEvent) -> Vec<UiEvent> {
    match event {
        ReaderEvent::Terminal(CtEvent::Key(key)) if key.kind == KeyEventKind::Press => {
            vec![UiEvent::Key(key)]
        }
        ReaderEvent::Terminal(CtEvent::Paste(text)) => vec![UiEvent::Paste(text)],
        ReaderEvent::Terminal(CtEvent::Mouse(mouse)) => match mouse.kind {
            MouseEventKind::ScrollUp => vec![UiEvent::Scroll(-3)],
            MouseEventKind::ScrollDown => vec![UiEvent::Scroll(3)],
            _ => Vec::new(),
        },
        ReaderEvent::Terminal(CtEvent::Resize(_, _)) => Vec::new(),
        ReaderEvent::Terminal(_) => Vec::new(),
        ReaderEvent::Error(error) => vec![UiEvent::Entry(Entry::Error(error))],
    }
}
