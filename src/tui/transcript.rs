use crate::tui::theme;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;
use std::collections::HashSet;
use tiny::Message;

pub(crate) enum Entry {
    User(String),
    Assistant(String),
    ToolCall { name: String, args: Value },
    ToolResult { content: String, is_error: bool },
    Error(String),
}

const COLLAPSED_TOOL_RESULT_LINES: usize = 3;

#[derive(Default)]
pub(crate) struct Transcript {
    entries: Vec<Entry>,
    // Indices of entries (currently only ToolResult) the user has expanded.
    // Stable across push() — reset on clear().
    expanded: HashSet<usize>,
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

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.expanded.clear();
        self.relayout();
    }

    pub(crate) fn line_at(&self, index: usize) -> Option<Line<'static>> {
        self.lines.get(index).cloned()
    }

    pub(crate) fn last_tool_result_index(&self) -> Option<usize> {
        self.entries
            .iter()
            .rposition(|entry| matches!(entry, Entry::ToolResult { .. }))
    }

    pub(crate) fn toggle_expanded(&mut self, index: usize) {
        if !self.expanded.insert(index) {
            self.expanded.remove(&index);
        }
        self.relayout();
    }

    fn relayout(&mut self) {
        self.lines.clear();
        if self.width == 0 {
            return;
        }

        for (index, entry) in self.entries.iter().enumerate() {
            for line in render_entry(index, entry, &self.expanded) {
                wrap_line(line, self.width, &mut self.lines);
            }
        }

        self.lines.push(Line::default());
    }
}

pub(crate) fn entries_from_message(message: Message) -> Vec<Entry> {
    match message {
        Message::User(text) => vec![Entry::User(text)],
        Message::Assistant { text, tool_calls } => {
            let mut out = Vec::new();
            if !text.is_empty() {
                out.push(Entry::Assistant(text));
            }
            out.extend(tool_calls.into_iter().map(|call| Entry::ToolCall {
                name: call.name,
                args: call.input,
            }));
            out
        }
        Message::Tool(result) => vec![Entry::ToolResult {
            content: result.content,
            is_error: result.is_error,
        }],
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

fn render_entry(index: usize, entry: &Entry, expanded: &HashSet<usize>) -> Vec<Line<'static>> {
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
            render_assistant(text, &mut lines);
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
            render_tool_result(content, *is_error, expanded.contains(&index), &mut lines);
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

fn render_tool_result(content: &str, is_error: bool, expanded: bool, out: &mut Vec<Line<'static>>) {
    let body_style = if is_error {
        Style::default().fg(theme::ERROR)
    } else {
        Style::default().fg(theme::DIM)
    };
    let dim = Style::default().fg(theme::DIM);

    let body_lines: Vec<&str> = content.lines().map(|line| line.trim_end()).collect();
    let total = body_lines.len();
    let shown = if expanded {
        total
    } else {
        total.min(COLLAPSED_TOOL_RESULT_LINES)
    };

    for (i, line) in body_lines.iter().take(shown).enumerate() {
        let arrow = if i == 0 { "  -> " } else { "     " };
        out.push(Line::from(vec![
            Span::raw(theme::GUTTER),
            Span::styled(arrow, dim),
            Span::styled((*line).to_string(), body_style),
        ]));
    }

    let hint = if expanded && total > COLLAPSED_TOOL_RESULT_LINES {
        Some("     tab to collapse".to_string())
    } else if !expanded && total > shown {
        Some(format!(
            "     +{} more line{} · tab to expand",
            total - shown,
            if total - shown == 1 { "" } else { "s" }
        ))
    } else {
        None
    };

    if let Some(hint) = hint {
        out.push(Line::from(vec![
            Span::raw(theme::GUTTER),
            Span::styled(hint, dim),
        ]));
    }
}

fn render_assistant(text: &str, out: &mut Vec<Line<'static>>) {
    let dim = Style::default().fg(theme::DIM);
    let assistant_marker_style = Style::default()
        .fg(theme::ASSISTANT)
        .add_modifier(Modifier::BOLD);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mut in_code = false;
    let mut first_line_emitted = false;

    for raw in text.lines() {
        let trimmed = raw.trim_start();
        let is_fence = trimmed.starts_with("```");

        let prefix_span = |first: bool| {
            if first {
                Span::styled("· ", assistant_marker_style)
            } else {
                Span::raw("  ")
            }
        };

        if is_fence {
            let first = !first_line_emitted;
            first_line_emitted = true;
            out.push(Line::from(vec![
                Span::raw(theme::GUTTER),
                prefix_span(first),
                Span::styled(raw.to_string(), dim),
            ]));
            in_code = !in_code;
            continue;
        }

        if in_code {
            let first = !first_line_emitted;
            first_line_emitted = true;
            out.push(Line::from(vec![
                Span::raw(theme::GUTTER),
                prefix_span(first),
                Span::styled(raw.to_string(), dim),
            ]));
            continue;
        }

        let first = !first_line_emitted;
        first_line_emitted = true;
        let mut spans = vec![Span::raw(theme::GUTTER), prefix_span(first)];

        if let Some(rest) = heading_body(raw) {
            spans.push(Span::styled(rest.to_string(), bold));
        } else if let Some(rest) = bullet_body(raw) {
            spans.push(Span::raw("• "));
            extend_with_inline_code(&mut spans, rest);
        } else {
            extend_with_inline_code(&mut spans, raw);
        }

        out.push(Line::from(spans));
    }
}

fn heading_body(line: &str) -> Option<&str> {
    let mut hashes = 0;
    let bytes = line.as_bytes();
    while hashes < bytes.len() && bytes[hashes] == b'#' {
        hashes += 1;
    }
    if hashes == 0 || hashes > 3 {
        return None;
    }
    if bytes.get(hashes) == Some(&b' ') {
        Some(&line[hashes + 1..])
    } else {
        None
    }
}

fn bullet_body(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'-' || bytes[0] == b'*') && bytes[1] == b' ' {
        Some(&line[2..])
    } else {
        None
    }
}

fn extend_with_inline_code(spans: &mut Vec<Span<'static>>, text: &str) {
    let code_style = Style::default().fg(theme::TOOL);
    let mut in_code = false;
    let mut buf = String::new();

    for c in text.chars() {
        if c == '`' {
            if !buf.is_empty() {
                if in_code {
                    spans.push(Span::styled(std::mem::take(&mut buf), code_style));
                } else {
                    spans.push(Span::raw(std::mem::take(&mut buf)));
                }
            }
            in_code = !in_code;
            continue;
        }
        buf.push(c);
    }

    if !buf.is_empty() {
        if in_code {
            // Unbalanced trailing backtick: emit as literal.
            let mut literal = String::from("`");
            literal.push_str(&buf);
            spans.push(Span::raw(literal));
        } else {
            spans.push(Span::raw(buf));
        }
    }
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
