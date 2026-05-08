use crate::backend;
use crate::tui::prompt::Prompt;
use crate::tui::{
    events::handle_backend_event, keys::handle_input_event, print, reader, render::render_screen,
    state::AppState,
};
use anyhow::Result;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tiny::AgentConfig;
use tokio::sync::mpsc;

const ANIMATION_INTERVAL: Duration = Duration::from_millis(80);

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
    let mut animation = tokio::time::interval(ANIMATION_INTERVAL);
    animation.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let cwd = std::env::current_dir().unwrap_or_default();
    print::print_intro(out, &model, &cwd.display().to_string())?;
    out.flush()?;
    render_screen(out, &mut prompt, &state)?;

    loop {
        let needs_render = tokio::select! {
            biased;
            event = reader_rx.recv() => {
                let Some(event) = event else { break };
                let mut should_quit = handle_input_event(out, &mut prompt, &mut state, &backend, event)?;
                while !should_quit {
                    let Ok(event) = reader_rx.try_recv() else {
                        break;
                    };
                    should_quit = handle_input_event(out, &mut prompt, &mut state, &backend, event)?;
                }
                if should_quit {
                    break;
                }
                true
            }
            Some(event) = backend.events.recv() => {
                handle_backend_event(out, &mut prompt, &mut state, event)?;
                while let Ok(event) = backend.events.try_recv() {
                    handle_backend_event(out, &mut prompt, &mut state, event)?;
                }
                true
            }
            _ = animation.tick(), if state.is_busy() => {
                true
            }
            else => break,
        };
        if needs_render {
            render_screen(out, &mut prompt, &state)?;
        }
    }

    prompt.clear(out)?;
    out.flush()?;
    Ok(())
}
