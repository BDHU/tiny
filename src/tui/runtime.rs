use crate::{
    backend::{self, BackendCommand, BackendEvent},
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
use tiny::{Agent, Message};
use tokio::sync::mpsc;

pub(crate) async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    agent: Agent,
    model: String,
) -> Result<()> {
    let mut state = State::new(model);
    let mut backend = backend::spawn(agent);
    let (reader_tx, mut reader_rx) = mpsc::unbounded_channel();
    let _reader = reader::spawn(reader_tx);
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(80));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    draw(terminal, &mut state)?;
    loop {
        let redraw = tokio::select! {
            _ = ticker.tick() => {
                if state.turn.busy {
                    state::update(&mut state, UiEvent::Tick);
                    true
                } else {
                    false
                }
            }

            Some(event) = backend.events.recv() => {
                let effects = handle_backend_event(&mut state, event);
                if !apply_effects(terminal, &backend.commands, effects)? {
                    break;
                }
                true
            }

            Some(event) = reader_rx.recv() => {
                let mut redraw = false;
                let mut keep_running = true;
                if !process_reader_event(terminal, &backend.commands, &mut state, event, &mut redraw)? {
                    keep_running = false;
                }
                while keep_running {
                    let Ok(event) = reader_rx.try_recv() else {
                        break;
                    };
                    if !process_reader_event(terminal, &backend.commands, &mut state, event, &mut redraw)? {
                        keep_running = false;
                    }
                }
                if !keep_running {
                    break;
                }
                redraw
            }
        };

        if redraw {
            draw(terminal, &mut state)?;
        }
    }

    let _ = backend.commands.send(BackendCommand::Shutdown);
    Ok(())
}

fn process_reader_event(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    commands: &mpsc::UnboundedSender<BackendCommand>,
    state: &mut State,
    event: ReaderEvent,
    redraw: &mut bool,
) -> Result<bool> {
    let Some(effects) = handle_reader_event(state, event) else {
        return Ok(true);
    };
    if !apply_effects(terminal, commands, effects)? {
        return Ok(false);
    }
    *redraw = true;
    Ok(true)
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut State,
) -> Result<()> {
    refresh_viewport(terminal, state)?;
    terminal.draw(|f| view::ui(f, state))?;
    Ok(())
}

fn refresh_viewport(
    terminal: &Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut State,
) -> Result<()> {
    let size = terminal.size()?;
    let area = Rect::new(0, 0, size.width, size.height);
    let message_area = view::message_area(area);
    state::update(
        state,
        UiEvent::Viewport {
            width: message_area.width,
            height: message_area.height,
        },
    );
    Ok(())
}

fn handle_reader_event(state: &mut State, event: ReaderEvent) -> Option<Vec<Effect>> {
    match event {
        ReaderEvent::Terminal(CtEvent::Key(key)) if key.kind == KeyEventKind::Press => {
            Some(state::update(state, UiEvent::Key(key)))
        }
        ReaderEvent::Terminal(CtEvent::Paste(text)) => {
            Some(state::update(state, UiEvent::Paste(text)))
        }
        ReaderEvent::Terminal(CtEvent::Mouse(mouse)) => match mouse.kind {
            MouseEventKind::ScrollUp => Some(state::update(state, UiEvent::Scroll(-3))),
            MouseEventKind::ScrollDown => Some(state::update(state, UiEvent::Scroll(3))),
            _ => None,
        },
        ReaderEvent::Terminal(CtEvent::Resize(_, _)) => Some(Vec::new()),
        ReaderEvent::Terminal(_) => None,
        ReaderEvent::Error(error) => {
            Some(state::update(state, UiEvent::Entry(Entry::Error(error))))
        }
    }
}

fn handle_backend_event(state: &mut State, event: BackendEvent) -> Vec<Effect> {
    match event {
        BackendEvent::Message(Message::User(_)) => Vec::new(),
        BackendEvent::Message(message) => {
            for entry in entries_from_message(&message) {
                state::update(state, UiEvent::Entry(entry));
            }
            Vec::new()
        }
        BackendEvent::PermissionRequest { id, call } => {
            state::update(state, UiEvent::PermissionRequest { id, call })
        }
        BackendEvent::TurnStarted => state::update(state, UiEvent::TurnStarted),
        BackendEvent::TurnError(error) => state::update(state, UiEvent::TurnError(error)),
        BackendEvent::TurnDone => state::update(state, UiEvent::TurnDone),
    }
}

fn apply_effects(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    commands: &mpsc::UnboundedSender<BackendCommand>,
    effects: Vec<Effect>,
) -> Result<bool> {
    for effect in effects {
        match effect {
            Effect::Quit => return Ok(false),
            Effect::Submit(input) => {
                let _ = commands.send(BackendCommand::Submit(input));
            }
            Effect::ReplyPermission { id, decision } => {
                let _ = commands.send(BackendCommand::PermissionDecision { id, decision });
            }
            Effect::Redraw => {
                terminal.clear()?;
            }
        }
    }
    Ok(true)
}

fn entries_from_message(message: &Message) -> Vec<Entry> {
    match message {
        Message::User(text) => vec![Entry::User(text.clone())],
        Message::Assistant { text, tool_calls } => {
            let mut entries = Vec::new();
            if !text.is_empty() {
                entries.push(Entry::Assistant(text.clone()));
            }
            entries.extend(tool_calls.iter().map(|call| Entry::ToolCall {
                name: call.name.clone(),
                args: call.input.clone(),
            }));
            entries
        }
        Message::Tool(result) => vec![Entry::ToolResult {
            content: result.content.clone(),
            is_error: result.is_error,
        }],
    }
}
