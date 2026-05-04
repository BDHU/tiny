use anyhow::Result;
use crossterm::{
    event::{Event as CtEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
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
use std::{
    io::{stdin, stdout, IsTerminal},
    time::Duration,
};
use tiny::{Agent, Event};
use tokio::sync::mpsc;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

enum Entry {
    User(String),
    Assistant(String),
    ToolCall { name: String, args: Value },
    ToolResult { content: String, is_error: bool },
    Error(String),
}

enum Action {
    Quit,
    Submit(String),
}

enum TurnEvent {
    Entry(Entry),
    Done,
}

struct App {
    input: String,
    cursor: usize,
    entries: Vec<Entry>,
    scroll: u16,
    auto_scroll: bool,
    tick: usize,
    model: String,
    waiting: bool,
}

impl Entry {
    fn height(&self) -> u16 {
        match self {
            Entry::User(_) | Entry::Error(_) => 2,
            Entry::Assistant(text) => text.lines().count() as u16 + 1,
            Entry::ToolCall { .. } | Entry::ToolResult { .. } => 1,
        }
    }

    fn render(&self, lines: &mut Vec<Line<'static>>) {
        match self {
            Entry::User(text) => {
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(
                    format!("You: {text}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
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
                let preview = preview(&arg_str, 60);
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
                let preview = result_preview(content);
                lines.push(Line::from(Span::styled(format!("  → {preview}"), style)));
            }
            Entry::Error(text) => {
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(
                    format!("Error: {text}"),
                    Style::default().fg(Color::Red),
                )));
            }
        }
    }
}

impl App {
    fn new(model: String) -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            entries: Vec::new(),
            scroll: 0,
            auto_scroll: false,
            tick: 0,
            model,
            waiting: false,
        }
    }

    fn content_height(&self) -> u16 {
        self.entries.iter().map(Entry::height).sum()
    }

    fn scroll_to_bottom(&mut self, viewport_height: u16) {
        self.scroll = self.content_height().saturating_sub(viewport_height);
        self.auto_scroll = false;
    }

    fn push_entry(&mut self, entry: Entry) {
        self.entries.push(entry);
        self.auto_scroll = true;
    }

    fn clear_input(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.input)
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.move_left();
        self.input.remove(self.cursor);
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        while !self.input.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        self.cursor += 1;
        while !self.input.is_char_boundary(self.cursor) {
            self.cursor += 1;
        }
    }

    fn cursor_column(&self) -> u16 {
        self.input[..self.cursor].chars().count() as u16
    }
}

struct TerminalSession(Terminal<CrosstermBackend<std::io::Stdout>>);

impl TerminalSession {
    fn enter() -> Result<Self> {
        if !stdin().is_terminal() || !stdout().is_terminal() {
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

fn preview(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

fn result_preview(content: &str) -> String {
    let mut lines = content.lines();
    let preview = lines.by_ref().take(2).collect::<Vec<_>>().join(" • ");
    if lines.next().is_some() {
        format!("{preview} …")
    } else {
        preview
    }
}

fn transcript_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for entry in &app.entries {
        entry.render(&mut lines);
    }

    if app.waiting {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("  {} thinking…", SPINNER[app.tick % SPINNER.len()]),
            Style::default().fg(Color::Yellow),
        )));
    }

    lines
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let [msg_area, input_area, status_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(
        Paragraph::new(transcript_lines(app))
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        msg_area,
    );

    let prompt = format!("> {}", app.input);
    f.render_widget(
        Paragraph::new(prompt).block(Block::default().borders(Borders::ALL)),
        input_area,
    );
    f.set_cursor_position((input_area.x + 3 + app.cursor_column(), input_area.y + 1));

    let msg_count = app
        .entries
        .iter()
        .filter(|e| matches!(e, Entry::User(_) | Entry::Assistant(_)))
        .count();
    let cwd = std::env::current_dir().unwrap_or_default();
    f.render_widget(
        Paragraph::new(format!(
            " {} • {} messages • {}  (Ctrl+D to exit)",
            app.model,
            msg_count,
            cwd.display()
        ))
        .style(Style::default().fg(Color::DarkGray)),
        status_area,
    );
}

fn handle_key(app: &mut App, key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),

        KeyCode::Enter if !app.waiting && !app.input.trim().is_empty() => {
            Some(Action::Submit(app.clear_input()))
        }

        KeyCode::Char(c) if !app.waiting => {
            app.insert_char(c);
            None
        }
        KeyCode::Backspace if !app.waiting => {
            app.backspace();
            None
        }
        KeyCode::Left => {
            app.move_left();
            None
        }
        KeyCode::Right => {
            app.move_right();
            None
        }
        KeyCode::Esc => {
            app.clear_input();
            None
        }
        KeyCode::PageUp => {
            app.scroll = app.scroll.saturating_sub(10);
            None
        }
        KeyCode::PageDown => {
            app.scroll = app.scroll.saturating_add(10);
            None
        }
        KeyCode::Up => {
            app.scroll = app.scroll.saturating_sub(1);
            None
        }
        KeyCode::Down => {
            app.scroll = app.scroll.saturating_add(1);
            None
        }
        _ => None,
    }
}

// Long-lived task: owns the Agent for the whole session. Receives user inputs
// on `inputs`, streams Entries back through `events` as they happen, and signals
// `Done` after each turn so the UI can re-enable input.
async fn drive_turns(
    mut agent: Agent,
    mut inputs: mpsc::Receiver<String>,
    events: mpsc::UnboundedSender<TurnEvent>,
) {
    while let Some(input) = inputs.recv().await {
        let tx = events.clone();
        let result = agent
            .send(input, |event| {
                let entry = match event {
                    Event::AssistantText(text) => Entry::Assistant(text.clone()),
                    Event::ToolCall { name, input, .. } => Entry::ToolCall {
                        name: name.clone(),
                        args: input.clone(),
                    },
                    Event::ToolResult {
                        content, is_error, ..
                    } => Entry::ToolResult {
                        content: content.clone(),
                        is_error: *is_error,
                    },
                };
                let _ = tx.send(TurnEvent::Entry(entry));
            })
            .await;

        if let Err(e) = result {
            let _ = events.send(TurnEvent::Entry(Entry::Error(e.to_string())));
        }
        let _ = events.send(TurnEvent::Done);
    }
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    inputs: mpsc::Sender<String>,
    mut events: mpsc::UnboundedReceiver<TurnEvent>,
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

            Some(event) = events.recv() => match event {
                TurnEvent::Entry(entry) => app.push_entry(entry),
                TurnEvent::Done => app.waiting = false,
            },

            Some(Ok(key_event)) = keys.next() => {
                if let CtEvent::Key(key) = key_event {
                    if key.kind != KeyEventKind::Press { continue; }

                    match handle_key(app, key) {
                        Some(Action::Quit) => break,
                        Some(Action::Submit(input)) => {
                            app.push_entry(Entry::User(input.clone()));
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
    let mut app = App::new(model);
    let mut terminal = TerminalSession::enter()?;

    let (input_tx, input_rx) = mpsc::channel(1);
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(drive_turns(agent, input_rx, event_tx));

    event_loop(&mut terminal.0, &mut app, input_tx, event_rx).await
}
