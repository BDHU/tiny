// Renders the prompt area (input bar + optional menu/picker/status) at the
// bottom of the terminal, anchored just below the scrollback. The terminal
// stays in raw mode without an alternate screen, so chat history lives in
// the terminal's own scrollback and we only manage these few rows.

use crate::tui::{
    commands::{self, Command},
    input::InputBuffer,
    picker::SessionPicker,
    print,
    theme,
};
use crossterm::{
    cursor::{MoveToColumn, MoveToNextLine, MoveUp},
    queue,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::{self, Write};
use std::time::Instant;
use tiny::ToolCall;

#[derive(Default)]
pub(crate) struct Prompt {
    rendered_rows: u16,
    cursor_offset_from_top: u16,
}

pub(crate) struct View<'a> {
    pub(crate) input: &'a InputBuffer,
    pub(crate) palette_index: usize,
    pub(crate) picker: Option<&'a SessionPicker>,
    pub(crate) pending: Option<&'a ToolCall>,
    pub(crate) busy: bool,
    pub(crate) busy_started_at: Option<Instant>,
    pub(crate) tick: usize,
    pub(crate) queued: usize,
    pub(crate) model: &'a str,
    pub(crate) directory_label: &'a str,
    pub(crate) message_count: usize,
    pub(crate) active_session_id: Option<&'a str>,
    pub(crate) terminal_size: (u16, u16),
}

impl Prompt {
    pub(crate) fn clear<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        if self.cursor_offset_from_top > 0 {
            queue!(out, MoveUp(self.cursor_offset_from_top))?;
        }
        queue!(out, MoveToColumn(0), Clear(ClearType::FromCursorDown))?;
        self.rendered_rows = 0;
        self.cursor_offset_from_top = 0;
        Ok(())
    }

    pub(crate) fn render<W: Write>(&mut self, out: &mut W, view: &View<'_>) -> io::Result<()> {
        let (term_cols, term_rows) = view.terminal_size;
        let rows = build_rows(view, term_cols, term_rows);
        let total = rows.lines.len() as u16;

        // Wipe whatever the previous render left on screen so a shrinking
        // prompt does not leave stale rows behind.
        self.clear(out)?;

        if total == 0 {
            return Ok(());
        }

        // Allocate rows below current cursor: terminal scrolls if needed.
        if total > 1 {
            for _ in 0..(total - 1) {
                queue!(out, Print("\r\n"))?;
            }
            queue!(out, MoveUp(total - 1))?;
        }
        queue!(out, MoveToColumn(0))?;

        for (i, line) in rows.lines.iter().enumerate() {
            line.write(out)?;
            if i + 1 < rows.lines.len() {
                queue!(out, MoveToNextLine(1))?;
            }
        }

        // Cursor was left on the last row, move up to the input row.
        let from_bottom = (rows.lines.len() - 1 - rows.input_row) as u16;
        if from_bottom > 0 {
            queue!(out, MoveUp(from_bottom))?;
        }
        queue!(out, MoveToColumn(rows.input_col))?;

        out.flush()?;

        self.rendered_rows = total;
        self.cursor_offset_from_top = rows.input_row as u16;
        Ok(())
    }
}

struct Rows {
    lines: Vec<Row>,
    input_row: usize,
    input_col: u16,
}

enum Row {
    Static(Vec<Segment>),
}

struct Segment {
    text: String,
    fg: Option<crossterm::style::Color>,
    bold: bool,
}

impl Row {
    fn write<W: Write>(&self, out: &mut W) -> io::Result<()> {
        match self {
            Row::Static(segments) => {
                for seg in segments {
                    if let Some(fg) = seg.fg {
                        queue!(out, SetForegroundColor(fg))?;
                    }
                    if seg.bold {
                        queue!(out, SetAttribute(Attribute::Bold))?;
                    }
                    queue!(out, Print(&seg.text))?;
                    if seg.bold {
                        queue!(out, SetAttribute(Attribute::NormalIntensity))?;
                    }
                    if seg.fg.is_some() {
                        queue!(out, ResetColor)?;
                    }
                }
                Ok(())
            }
        }
    }
}

fn seg(text: impl Into<String>) -> Segment {
    Segment {
        text: text.into(),
        fg: None,
        bold: false,
    }
}

fn fg(text: impl Into<String>, color: crossterm::style::Color) -> Segment {
    Segment {
        text: text.into(),
        fg: Some(color),
        bold: false,
    }
}

fn bold_fg(text: impl Into<String>, color: crossterm::style::Color) -> Segment {
    Segment {
        text: text.into(),
        fg: Some(color),
        bold: true,
    }
}

fn build_rows(view: &View<'_>, term_cols: u16, term_rows: u16) -> Rows {
    let mut lines: Vec<Row> = Vec::new();

    // Picker (above everything else; takes precedence over palette)
    let picker_active = view.picker.is_some();
    if let Some(picker) = view.picker {
        push_picker(&mut lines, picker, view.active_session_id, term_cols, term_rows);
    }

    // Spinner (only when busy and no permission prompt)
    if view.busy && view.pending.is_none() {
        lines.push(spinner_row(view));
    }

    // Input or permission row
    let (input_row_idx, input_col) = if let Some(call) = view.pending {
        lines.push(permission_row(call, term_cols));
        (lines.len() - 1, 0)
    } else {
        let (row, col) = input_row(view, term_cols);
        lines.push(row);
        (lines.len() - 1, col)
    };

    // Palette (slash-command completions) appears below the input bar.
    if !picker_active {
        push_palette(&mut lines, view);
    }

    // Status row (always last)
    lines.push(status_row(view, term_cols));

    Rows {
        lines,
        input_row: input_row_idx,
        input_col,
    }
}

fn input_row(view: &View<'_>, term_cols: u16) -> (Row, u16) {
    let prefix = "> ";
    let prefix_cols = prefix.chars().count() as u16;
    let inner_width = term_cols.saturating_sub(prefix_cols).max(1);
    let cursor_col_full = view.input.cursor_column();
    let scroll_x = cursor_col_full.saturating_sub(inner_width.saturating_sub(1));

    let visible: String = view
        .input
        .as_str()
        .chars()
        .skip(scroll_x as usize)
        .take(inner_width as usize)
        .collect();

    let prefix_color = if view.busy { theme::DIM } else { theme::USER };
    let row = Row::Static(vec![
        bold_fg(prefix, prefix_color),
        seg(visible),
    ]);
    let cursor_col = prefix_cols + (cursor_col_full - scroll_x);
    (row, cursor_col)
}

fn permission_row(call: &ToolCall, _term_cols: u16) -> Row {
    let text = format!(
        " Run {}({})?  [y]es  [n]o ",
        call.name,
        print::preview(&call.input.to_string(), 60)
    );
    Row::Static(vec![bold_fg(text, theme::TOOL)])
}

fn spinner_row(view: &View<'_>) -> Row {
    let elapsed = view
        .busy_started_at
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let frame = theme::SPINNER[view.tick % theme::SPINNER.len()];
    Row::Static(vec![
        seg(theme::GUTTER),
        fg(frame.to_string(), theme::TOOL),
        fg(format!(" Thinking... ({elapsed}s)"), theme::DIM),
    ])
}

fn status_row(view: &View<'_>, term_cols: u16) -> Row {
    let queued = if view.queued == 0 {
        String::new()
    } else {
        format!(" · {} queued", view.queued)
    };
    let left = format!(
        " {} · {} msgs{} · {}",
        view.model, view.message_count, queued, view.directory_label
    );
    let right = "⏎ send · /quit ";
    let left_cols = left.chars().count() as u16;
    let right_cols = right.chars().count() as u16;
    let pad = term_cols
        .saturating_sub(left_cols)
        .saturating_sub(right_cols);
    let mut text = left;
    text.push_str(&" ".repeat(pad as usize));
    text.push_str(right);
    Row::Static(vec![fg(text, theme::DIM)])
}

fn push_palette(lines: &mut Vec<Row>, view: &View<'_>) {
    let matches = commands::palette_matches(view.input.as_str());
    if matches.is_empty() {
        return;
    }
    let selected = view.palette_index.min(matches.len() - 1);
    let name_width = matches.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for (i, cmd) in matches.iter().enumerate() {
        lines.push(palette_row(cmd, name_width, i == selected));
    }
}

fn palette_row(cmd: &Command, name_width: usize, selected: bool) -> Row {
    let pad = " ".repeat(name_width.saturating_sub(cmd.name.len()));
    let marker = if selected { " > " } else { "   " };
    let segs = if selected {
        vec![
            bold_fg(marker, theme::USER),
            bold_fg(format!("/{}", cmd.name), theme::USER),
            seg(format!("{pad}  ")),
            fg(cmd.help.to_string(), theme::DIM),
        ]
    } else {
        vec![
            fg(marker, theme::DIM),
            seg(format!("/{}", cmd.name)),
            seg(format!("{pad}  ")),
            fg(cmd.help.to_string(), theme::DIM),
        ]
    };
    Row::Static(segs)
}

fn push_picker(
    lines: &mut Vec<Row>,
    picker: &SessionPicker,
    active_id: Option<&str>,
    _term_cols: u16,
    term_rows: u16,
) {
    if picker.sessions.is_empty() {
        return;
    }
    // Cap picker height so input/status still fit on screen.
    let max_rows = term_rows.saturating_sub(5).min(20).max(1) as usize;
    let total = picker.sessions.len();
    let window = max_rows.min(total);
    let selected = picker.selected.min(total - 1);
    let start = if selected >= window { selected + 1 - window } else { 0 };
    let end = (start + window).min(total);

    lines.push(Row::Static(vec![fg(
        " Sessions ".to_string(),
        theme::DIM,
    )]));

    for i in start..end {
        let meta = &picker.sessions[i];
        let is_selected = i == selected;
        let is_active = Some(meta.id.as_str()) == active_id;
        let marker = if is_selected { ">" } else { " " };
        let active_marker = if is_active { "*" } else { " " };
        let title = if meta.title.is_empty() {
            "(untitled)"
        } else {
            meta.title.as_str()
        };
        let prefix = format!(" {marker}{active_marker} ");
        let segs = if is_selected {
            vec![bold_fg(prefix, theme::USER), bold_fg(title.to_string(), theme::USER)]
        } else {
            vec![seg(prefix), seg(title.to_string())]
        };
        lines.push(Row::Static(segs));
    }

    lines.push(Row::Static(vec![fg(
        " ↑/↓ navigate  ⏎ resume  esc cancel ".to_string(),
        theme::DIM,
    )]));
}

