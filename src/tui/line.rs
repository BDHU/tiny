use crate::tui::transcript::{preview, result_preview};
use anyhow::Result;
use tiny::{Agent, Decision, Event, Message};
use tokio::io::{self, AsyncBufReadExt};
use tokio::sync::mpsc;

pub(crate) async fn line_mode(mut agent: Agent, model: String) -> Result<()> {
    eprintln!("{model} line mode. Type a message and press Enter. Ctrl+D exits.");

    let mut lines = io::BufReader::new(io::stdin()).lines();
    let mut saw_stdin = false;

    while let Some(input) = lines.next_line().await? {
        saw_stdin = true;
        if input.trim().is_empty() {
            continue;
        }

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let turn = agent.send(input, &event_tx);
        tokio::pin!(turn);

        loop {
            tokio::select! {
                result = &mut turn => {
                    let _ = result;
                    break;
                }
                Some(event) = event_rx.recv() => handle_line_event(event),
            }
        }

        while let Ok(event) = event_rx.try_recv() {
            handle_line_event(event);
        }
    }

    if !saw_stdin {
        anyhow::bail!(
            "stdin is closed; run from an interactive terminal for the TUI, or pipe a prompt into line mode"
        );
    }

    Ok(())
}

fn handle_line_event(event: Event) {
    match event {
        Event::Message(message) => match message {
            Message::User(_) => {}
            Message::Assistant { text, tool_calls } => {
                if !text.is_empty() {
                    println!("{text}");
                }
                for call in tool_calls {
                    eprintln!(
                        "tool: {}({})",
                        call.name,
                        preview(&call.input.to_string(), 80)
                    );
                }
            }
            Message::Tool(result) => {
                if result.is_error {
                    eprintln!("tool error: {}", result.content);
                } else {
                    eprintln!("tool result: {}", result_preview(&result.content));
                }
            }
        },
        Event::PermissionRequest { call, reply } => {
            let _ = reply.send(Decision::Deny(format!(
                "tool '{}' requires the interactive TUI for permission",
                call.name
            )));
        }
        Event::TurnError(error) => eprintln!("Error: {error}"),
        Event::TurnDone => {}
    }
}
