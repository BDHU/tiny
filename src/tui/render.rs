use crate::tui::app::{App, Entry};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl Entry {
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
                let preview = preview(&args.to_string(), 60);
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

pub(crate) fn preview(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

pub(crate) fn result_preview(content: &str) -> String {
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

pub(crate) fn ui(f: &mut ratatui::Frame, app: &App) {
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

    if let Some(pending) = &app.pending {
        let prompt = format!(
            "Allow {}({})?  [y]es  [n]o",
            pending.call.name,
            preview(&pending.call.input.to_string(), 60)
        );
        f.render_widget(
            Paragraph::new(prompt)
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL)),
            input_area,
        );
    } else {
        let prompt = format!("> {}", app.input);
        f.render_widget(
            Paragraph::new(prompt).block(Block::default().borders(Borders::ALL)),
            input_area,
        );
        f.set_cursor_position((input_area.x + 3 + app.cursor_column(), input_area.y + 1));
    }

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
