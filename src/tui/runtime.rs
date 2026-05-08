use crate::backend;
use crate::tui::prompt::Prompt;
use crate::tui::{
    events::handle_backend_event, keys::handle_input_event, print, reader, render::render_screen,
    state::AppState,
};
use anyhow::Result;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny::AgentConfig;
use tokio::sync::mpsc;

const TICK_INTERVAL: Duration = Duration::from_millis(80);
const INPUT_ACTIVITY_GRACE: Duration = Duration::from_millis(250);

pub(crate) async fn run<W: Write>(
    out: &mut W,
    config: Arc<AgentConfig>,
    model: String,
) -> Result<()> {
    let mut state = AppState::new();
    let mut prompt = Prompt::default();
    let mut backend = backend::spawn(config, model.clone());
    let (reader_tx, mut reader_rx) = mpsc::unbounded_channel();
    let _reader = reader::spawn(reader_tx);
    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_input_at: Option<Instant> = None;

    let cwd = std::env::current_dir().unwrap_or_default();
    print::print_intro(out, &model, &cwd.display().to_string())?;
    out.flush()?;
    render_screen(out, &mut prompt, &state)?;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let input_recent = last_input_at
                    .map(|at| at.elapsed() < INPUT_ACTIVITY_GRACE)
                    .unwrap_or(false);
                if state.is_busy() && !input_recent {
                    render_screen(out, &mut prompt, &state)?;
                }
            }
            Some(event) = backend.events.recv() => {
                handle_backend_event(out, &mut prompt, &mut state, event)?;
                while let Ok(event) = backend.events.try_recv() {
                    handle_backend_event(out, &mut prompt, &mut state, event)?;
                }
                render_screen(out, &mut prompt, &state)?;
            }
            event = reader_rx.recv() => {
                let Some(event) = event else { break };
                let mut should_quit = handle_input_event(out, &mut prompt, &mut state, &backend, event)?;
                while !should_quit {
                    let Ok(event) = reader_rx.try_recv() else {
                        break;
                    };
                    should_quit = handle_input_event(out, &mut prompt, &mut state, &backend, event)?;
                }
                last_input_at = Some(Instant::now());
                if should_quit {
                    break;
                }
                render_screen(out, &mut prompt, &state)?;
            }
            else => break,
        }
    }

    prompt.clear(out)?;
    out.flush()?;
    Ok(())
}
