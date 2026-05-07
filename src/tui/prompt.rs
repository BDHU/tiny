// Renders the prompt area (input bar + optional menu/picker/status) at the
// bottom of the terminal, anchored just below the scrollback. The terminal
// stays in raw mode without an alternate screen, so chat history lives in
// the terminal's own scrollback and we only manage these few rows.

use crate::tui::{
    commands::{self, Command},
    input::InputBuffer,
    picker::SessionPicker,
    print, theme,
};
use crossterm::{
    cursor::{MoveToColumn, MoveUp},
    queue,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::{self, Write};
use std::time::Instant;
use tiny::ToolCall;

#[derive(Default)]
pub(crate) struct Prompt {
    cursor_offset_from_top: u16,
}

pub(crate) struct View<'a> {
    pub(crate) input: &'a InputBuffer,
    pub(crate) palette_index: usize,
    pub(crate) picker: Option<&'a SessionPicker>,
    pub(crate) pending_call: Option<&'a ToolCall>,
    pub(crate) turn_started_at: Option<Instant>,
    pub(crate) queued: usize,
    pub(crate) model: &'a str,
    pub(crate) directory_label: &'a str,
    pub(crate) message_count: usize,
    pub(crate) active_session_id: Option<&'a str>,
    pub(crate) terminal_size: (u16, u16),
}

impl View<'_> {
    fn busy(&self) -> bool {
        self.turn_started_at.is_some()
    }
}

impl Prompt {
    pub(crate) fn clear<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        if self.cursor_offset_from_top > 0 {
            queue!(out, MoveUp(self.cursor_offset_from_top))?;
        }
        queue!(out, MoveToColumn(0), Clear(ClearType::FromCursorDown))?;
        self.cursor_offset_from_top = 0;
        Ok(())
    }

    pub(crate) fn render<W: Write>(&mut self, out: &mut W, view: &View<'_>) -> io::Result<()> {
        let (term_cols, term_rows) = view.terminal_size;

        // Render every row into a buffer so we can count rows up front and
        // pre-scroll the terminal once. `\r\n` separates rows; `start_row`
        // emits the separator and bumps the row counter.
        let mut buf: Vec<u8> = Vec::new();
        let mut rows: u16 = 0;
        let input_row;
        let input_col;

        if let Some(picker) = view.picker {
            write_picker(&mut buf, &mut rows, picker, view.active_session_id, term_rows)?;
        }

        if view.busy() && view.pending_call.is_none() {
            start_row(&mut buf, &mut rows)?;
            write_spinner(&mut buf, view)?;
        }

        if let Some(call) = view.pending_call {
            start_row(&mut buf, &mut rows)?;
            write_permission(&mut buf, call)?;
            input_row = rows - 1;
            input_col = 0;
        } else {
            start_row(&mut buf, &mut rows)?;
            input_col = write_input(&mut buf, view, term_cols)?;
            input_row = rows - 1;
        }

        if view.picker.is_none() {
            write_palette(&mut buf, &mut rows, view)?;
        }

        start_row(&mut buf, &mut rows)?;
        write_status(&mut buf, view, term_cols)?;

        // Wipe whatever the previous render left on screen so a shrinking
        // prompt does not leave stale rows behind.
        self.clear(out)?;

        // Allocate rows below current cursor: terminal scrolls if needed.
        if rows > 1 {
            for _ in 0..(rows - 1) {
                queue!(out, Print("\r\n"))?;
            }
            queue!(out, MoveUp(rows - 1))?;
        }
        queue!(out, MoveToColumn(0))?;

        out.write_all(&buf)?;

        // Cursor was left on the last row, move up to the input row.
        let from_bottom = rows - 1 - input_row;
        if from_bottom > 0 {
            queue!(out, MoveUp(from_bottom))?;
        }
        queue!(out, MoveToColumn(input_col))?;
        out.flush()?;

        self.cursor_offset_from_top = input_row;
        Ok(())
    }
}

fn start_row<W: Write>(out: &mut W, rows: &mut u16) -> io::Result<()> {
    if *rows > 0 {
        out.write_all(b"\r\n")?;
    }
    *rows += 1;
    Ok(())
}

fn write_input<W: Write>(out: &mut W, view: &View<'_>, term_cols: u16) -> io::Result<u16> {
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
    let prefix_color = if view.busy() { theme::DIM } else { theme::USER };

    queue!(
        out,
        SetForegroundColor(prefix_color),
        SetAttribute(Attribute::Bold),
        Print(prefix),
        SetAttribute(Attribute::NormalIntensity),
        ResetColor,
        Print(&visible),
    )?;

    Ok(prefix_cols + (cursor_col_full - scroll_x))
}

fn write_permission<W: Write>(out: &mut W, call: &ToolCall) -> io::Result<()> {
    let text = format!(
        " Run {}({})?  [y]es  [n]o ",
        call.name,
        print::preview(&call.input.to_string(), 60)
    );
    queue!(
        out,
        SetForegroundColor(theme::TOOL),
        SetAttribute(Attribute::Bold),
        Print(text),
        SetAttribute(Attribute::NormalIntensity),
        ResetColor,
    )
}

fn write_spinner<W: Write>(out: &mut W, view: &View<'_>) -> io::Result<()> {
    let started = view.turn_started_at;
    let elapsed_secs = started.map(|t| t.elapsed().as_secs()).unwrap_or(0);
    let elapsed_ticks = started.map(|t| t.elapsed().as_millis() / 80).unwrap_or(0) as usize;
    let frame = theme::SPINNER[elapsed_ticks % theme::SPINNER.len()];
    queue!(
        out,
        Print(theme::GUTTER),
        SetForegroundColor(theme::TOOL),
        Print(frame),
        ResetColor,
        SetForegroundColor(theme::DIM),
        Print(format!(" Thinking... ({elapsed_secs}s)")),
        ResetColor,
    )
}

fn write_status<W: Write>(out: &mut W, view: &View<'_>, term_cols: u16) -> io::Result<()> {
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
    queue!(out, SetForegroundColor(theme::DIM), Print(text), ResetColor)
}

fn write_palette<W: Write>(out: &mut W, rows: &mut u16, view: &View<'_>) -> io::Result<()> {
    let matches = commands::palette_matches(view.input.as_str());
    if matches.is_empty() {
        return Ok(());
    }
    let selected = view.palette_index.min(matches.len() - 1);
    let name_width = matches.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for (i, cmd) in matches.iter().enumerate() {
        start_row(out, rows)?;
        write_palette_row(out, cmd, name_width, i == selected)?;
    }
    Ok(())
}

fn write_palette_row<W: Write>(
    out: &mut W,
    cmd: &Command,
    name_width: usize,
    selected: bool,
) -> io::Result<()> {
    let pad = " ".repeat(name_width.saturating_sub(cmd.name.len()));
    let marker = if selected { " > " } else { "   " };
    if selected {
        queue!(
            out,
            SetForegroundColor(theme::USER),
            SetAttribute(Attribute::Bold),
            Print(marker),
            Print(format!("/{}", cmd.name)),
            SetAttribute(Attribute::NormalIntensity),
            ResetColor,
            Print(format!("{pad}  ")),
            SetForegroundColor(theme::DIM),
            Print(cmd.help),
            ResetColor,
        )
    } else {
        queue!(
            out,
            SetForegroundColor(theme::DIM),
            Print(marker),
            ResetColor,
            Print(format!("/{}", cmd.name)),
            Print(format!("{pad}  ")),
            SetForegroundColor(theme::DIM),
            Print(cmd.help),
            ResetColor,
        )
    }
}

fn write_picker<W: Write>(
    out: &mut W,
    rows: &mut u16,
    picker: &SessionPicker,
    active_id: Option<&str>,
    term_rows: u16,
) -> io::Result<()> {
    if picker.sessions.is_empty() {
        return Ok(());
    }
    // Cap picker height so input/status still fit on screen.
    let max_rows = term_rows.saturating_sub(5).min(20).max(1) as usize;
    let total = picker.sessions.len();
    let window = max_rows.min(total);
    let selected = picker.selected.min(total - 1);
    let start = if selected >= window {
        selected + 1 - window
    } else {
        0
    };
    let end = (start + window).min(total);

    start_row(out, rows)?;
    queue!(
        out,
        SetForegroundColor(theme::DIM),
        Print(" Sessions "),
        ResetColor,
    )?;

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

        start_row(out, rows)?;
        if is_selected {
            queue!(
                out,
                SetForegroundColor(theme::USER),
                SetAttribute(Attribute::Bold),
                Print(prefix),
                Print(title),
                SetAttribute(Attribute::NormalIntensity),
                ResetColor,
            )?;
        } else {
            queue!(out, Print(prefix), Print(title))?;
        }
    }

    start_row(out, rows)?;
    queue!(
        out,
        SetForegroundColor(theme::DIM),
        Print(" ↑/↓ navigate  ⏎ resume  esc cancel "),
        ResetColor,
    )?;
    Ok(())
}
