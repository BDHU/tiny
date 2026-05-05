use crate::tui::theme;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;

pub(crate) enum Entry {
    User(String),
    Assistant(String),
    ToolCall { name: String, args: Value },
    ToolResult { content: String, is_error: bool },
    Error(String),
}

#[derive(Default)]
pub(crate) struct Transcript {
    entries: Vec<Entry>,
    lines: Vec<Line<'static>>,
    width: u16,
}

impl Transcript {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn height(&self) -> u16 {
        self.lines.len().try_into().unwrap_or(u16::MAX)
    }

    pub(crate) fn message_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, Entry::User(_) | Entry::Assistant(_)))
            .count()
    }

    pub(crate) fn resize(&mut self, width: u16) {
        let width = width.max(1);
        if self.width == width {
            return;
        }

        self.width = width;
        self.relayout();
    }

    pub(crate) fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
        self.relayout();
    }

    pub(crate) fn line_at(&self, index: usize) -> Option<Line<'static>> {
        self.lines.get(index).cloned()
    }

    fn relayout(&mut self) {
        self.lines.clear();
        if self.width == 0 {
            return;
        }

        for entry in &self.entries {
            for line in render_entry(entry) {
                wrap_line(line, self.width, &mut self.lines);
            }
        }

        self.lines.push(Line::default());
    }
}

pub(crate) fn preview(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{short}...")
    } else {
        short
    }
}

pub(crate) fn result_preview(content: &str) -> String {
    let mut lines = content.lines();
    let head = lines.by_ref().take(2).collect::<Vec<_>>().join(" | ");
    if lines.next().is_some() {
        format!("{head} ...")
    } else {
        head
    }
}

fn render_entry(entry: &Entry) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    match entry {
        Entry::User(text) => {
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::raw(theme::GUTTER),
                Span::styled(
                    "> ",
                    Style::default()
                        .fg(theme::USER)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(text.clone(), Style::default().fg(theme::USER)),
            ]));
        }
        Entry::Assistant(text) => {
            lines.push(Line::default());
            for line in text.lines() {
                lines.push(Line::from(format!("{}{line}", theme::GUTTER)));
            }
        }
        Entry::ToolCall { name, args } => {
            let args_preview = preview(&args.to_string(), 60);
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::raw(theme::GUTTER),
                Span::styled("* ", Style::default().fg(theme::TOOL)),
                Span::styled(
                    name.clone(),
                    Style::default()
                        .fg(theme::TOOL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("({args_preview})"), Style::default().fg(theme::DIM)),
            ]));
        }
        Entry::ToolResult { content, is_error } => {
            let style = if *is_error {
                Style::default().fg(theme::ERROR)
            } else {
                Style::default().fg(theme::DIM)
            };
            let body = result_preview(content);
            lines.push(Line::from(vec![
                Span::raw(theme::GUTTER),
                Span::styled("  -> ", Style::default().fg(theme::DIM)),
                Span::styled(body, style),
            ]));
        }
        Entry::Error(text) => {
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::raw(theme::GUTTER),
                Span::styled(
                    "x ",
                    Style::default()
                        .fg(theme::ERROR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(text.clone(), Style::default().fg(theme::ERROR)),
            ]));
        }
    }
    lines
}

fn wrap_line(line: Line<'static>, width: u16, out: &mut Vec<Line<'static>>) {
    if line.spans.is_empty() {
        out.push(line);
        return;
    }

    let max = width.max(1) as usize;
    let mut current = Line {
        style: line.style,
        alignment: line.alignment,
        spans: Vec::new(),
    };
    let mut current_width = 0usize;

    for span in line.spans {
        let style = span.style;
        let mut buf = String::new();

        for c in span.content.chars() {
            let w = char_width(c);
            if current_width > 0 && current_width + w > max {
                if !buf.is_empty() {
                    push_span(&mut current, std::mem::take(&mut buf), style);
                }
                out.push(std::mem::replace(
                    &mut current,
                    Line {
                        style: line.style,
                        alignment: line.alignment,
                        spans: Vec::new(),
                    },
                ));
                current_width = 0;
            }

            buf.push(c);
            current_width += w;
        }

        if !buf.is_empty() {
            push_span(&mut current, buf, style);
        }
    }

    out.push(current);
}

fn push_span(line: &mut Line<'static>, content: String, style: Style) {
    if let Some(last) = line.spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push_str(&content);
            return;
        }
    }
    line.spans.push(Span::styled(content, style));
}

fn char_width(c: char) -> usize {
    if c == '\t' {
        4
    } else if c.is_control() {
        0
    } else {
        1
    }
}
