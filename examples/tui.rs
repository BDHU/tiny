use anyhow::{Context, Result};
use async_trait::async_trait;
use crossterm::{
    event::{Event as CtEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use serde_json::Value;
use std::{io::stdout, time::Duration};
use tiny::{Agent, Config, Decision, Event, OpenAiProvider, Tool};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── ReadTool ──────────────────────────────────────────────────────────────────

struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str { "read" }
    fn description(&self) -> &str { "Read the contents of a file from disk." }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        })
    }
    async fn call(&self, input: Value) -> Result<String> {
        let path = input["path"].as_str().context("missing path")?;
        Ok(std::fs::read_to_string(path)?)
    }
}

// ── conversation entries ──────────────────────────────────────────────────────

enum Entry {
    User(String),
    Assistant(String),
    ToolCall { name: String, args: Value },
    ToolResult { content: String, is_error: bool },
}

// ── app state ─────────────────────────────────────────────────────────────────

struct App {
    input: String,
    cursor: usize,                     // byte index
    entries: Vec<Entry>,
    scroll: u16,
    auto_scroll: bool,
    tick: usize,
    model: String,
    waiting: bool,
    // owned when idle; taken out while a turn is running
    agent: Option<Agent>,
}

// ── rendering ─────────────────────────────────────────────────────────────────

fn ui(f: &mut ratatui::Frame, app: &App) {
    let [msg_area, input_area, status_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ]).areas(f.area());

    // messages
    let mut lines: Vec<Line> = Vec::new();
    for entry in &app.entries {
        match entry {
            Entry::User(text) => {
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(
                    format!("You: {text}"),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )));
            }
            Entry::Assistant(text) => {
                lines.push(Line::default());
                for line in text.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
            Entry::ToolCall { name, args } => {
                let arg_str = args.to_string();
                let preview = if arg_str.len() > 60 { format!("{}…", &arg_str[..60]) } else { arg_str };
                lines.push(Line::from(Span::styled(
                    format!("  ⚙ {name}({preview})"),
                    Style::default().fg(Color::Yellow),
                )));
            }
            Entry::ToolResult { content, is_error } => {
                let style = if *is_error {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let preview = content.lines().take(2).collect::<Vec<_>>().join(" • ");
                let preview = if content.lines().count() > 2 {
                    format!("{preview} …")
                } else {
                    preview
                };
                lines.push(Line::from(Span::styled(format!("  → {preview}"), style)));
            }
        }
    }

    if app.waiting {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("  {} thinking…", SPINNER[app.tick % SPINNER.len()]),
            Style::default().fg(Color::Yellow),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.scroll, 0)),
        msg_area,
    );

    // input box
    let prompt = format!("> {}", app.input);
    f.render_widget(
        Paragraph::new(prompt).block(Block::default().borders(Borders::ALL)),
        input_area,
    );
    // cursor: +1 border, +2 for "> "
    f.set_cursor_position((
        input_area.x + 1 + 2 + app.cursor as u16,
        input_area.y + 1,
    ));

    // status bar
    let msg_count = app.entries.iter()
        .filter(|e| matches!(e, Entry::User(_) | Entry::Assistant(_)))
        .count();
    let cwd = std::env::current_dir().unwrap_or_default();
    f.render_widget(
        Paragraph::new(format!(
            " {} • {} messages • {}  (Ctrl+D to exit)",
            app.model, msg_count, cwd.display()
        )).style(Style::default().fg(Color::DarkGray)),
        status_area,
    );
}

// ── turn driver ───────────────────────────────────────────────────────────────

async fn run_turn(mut agent: Agent, input: String) -> (Agent, Result<Vec<Entry>>) {
    let mut entries = vec![Entry::User(input.clone())];
    let result = agent.send(input, |event| match event {
        Event::AssistantText(text) => entries.push(Entry::Assistant(text.clone())),
        Event::ToolCall { name, input, .. } => {
            entries.push(Entry::ToolCall { name: name.clone(), args: input.clone() });
        }
        Event::ToolResult { content, is_error, .. } => {
            entries.push(Entry::ToolResult { content: content.clone(), is_error: *is_error });
        }
    }).await;
    (agent, result.map(|_| entries))
}

// ── event loop ────────────────────────────────────────────────────────────────

async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let mut events = EventStream::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(Agent, Result<Vec<Entry>>)>(1);
    let mut ticker = tokio::time::interval(Duration::from_millis(80));

    loop {
        // auto-scroll to bottom when new content arrives
        if app.auto_scroll {
            let height = terminal.size()?.height.saturating_sub(4);
            let total: u16 = app.entries.iter().map(|e| match e {
                Entry::User(_)         => 2,
                Entry::Assistant(t)    => t.lines().count() as u16 + 1,
                Entry::ToolCall { .. } => 1,
                Entry::ToolResult { .. } => 1,
            }).sum();
            app.scroll = total.saturating_sub(height);
            app.auto_scroll = false;
        }

        terminal.draw(|f| ui(f, app))?;

        tokio::select! {
            _ = ticker.tick() => {
                app.tick = app.tick.wrapping_add(1);
            }

            Some((agent, result)) = rx.recv() => {
                app.agent = Some(agent);
                app.waiting = false;
                match result {
                    Ok(new_entries) => {
                        app.entries.extend(new_entries);
                        app.auto_scroll = true;
                    }
                    Err(e) => {
                        app.entries.push(Entry::Assistant(format!("Error: {e}")));
                        app.auto_scroll = true;
                    }
                }
            }

            Some(Ok(event)) = events.next() => {
                if let CtEvent::Key(key) = event {
                    if key.kind != KeyEventKind::Press { continue; }

                    match key.code {
                        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,

                        KeyCode::Enter if !app.waiting && !app.input.trim().is_empty() => {
                            if let Some(agent) = app.agent.take() {
                                let input  = std::mem::take(&mut app.input);
                                app.cursor = 0;
                                app.waiting = true;
                                let tx = tx.clone();
                                tokio::spawn(async move {
                                    let r = run_turn(agent, input).await;
                                    tx.send(r).await.ok();
                                });
                            }
                        }

                        KeyCode::Char(c) if !app.waiting => {
                            app.input.insert(app.cursor, c);
                            app.cursor += c.len_utf8();
                        }
                        KeyCode::Backspace if !app.waiting && app.cursor > 0 => {
                            app.cursor -= 1;
                            while !app.input.is_char_boundary(app.cursor) { app.cursor -= 1; }
                            app.input.remove(app.cursor);
                        }
                        KeyCode::Left if app.cursor > 0 => {
                            app.cursor -= 1;
                            while !app.input.is_char_boundary(app.cursor) { app.cursor -= 1; }
                        }
                        KeyCode::Right if app.cursor < app.input.len() => {
                            app.cursor += 1;
                            while !app.input.is_char_boundary(app.cursor) { app.cursor += 1; }
                        }
                        KeyCode::Esc => app.input.clear(),
                        KeyCode::PageUp   => app.scroll = app.scroll.saturating_sub(10),
                        KeyCode::PageDown => app.scroll = app.scroll.saturating_add(10),
                        KeyCode::Up       => app.scroll = app.scroll.saturating_sub(1),
                        KeyCode::Down     => app.scroll = app.scroll.saturating_add(1),
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(())
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::load()?;
    let api_key = cfg.api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .context("set api_key in tiny.json or OPENAI_API_KEY")?;
    let model = cfg.model.unwrap_or_else(|| "gpt-4o-mini".to_string());
    let system = cfg.system.unwrap_or_else(|| "You are a helpful assistant.".to_string());

    let mut agent = Agent::new(OpenAiProvider::new(&api_key, &model), system)
        .with_permission(|name, _| match name {
            "read" => Decision::Allow,
            other => Decision::Deny(format!("'{other}' not permitted")),
        });
    agent.register_tool(ReadTool);

    let mut app = App {
        input: String::new(),
        cursor: 0,
        entries: Vec::new(),
        scroll: 0,
        auto_scroll: false,
        tick: 0,
        model: model.clone(),
        waiting: false,
        agent: Some(agent),
    };

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}
