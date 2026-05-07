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

        // Render into a buffer first so we know how many rows to allocate.
        let mut frame = Frame::default();

        if view.busy() && view.pending_call.is_none() {
            frame.row()?;
            write_spinner(&mut frame, view)?;
        }

        // Blank row above the input/permission area for breathing room.
        frame.row()?;

        let (input_col, input_row) = if let Some(call) = view.pending_call {
            frame.row()?;
            write_permission(&mut frame, call, term_cols)?;
            (0, frame.current_row())
        } else {
            frame.row()?;
            let start_row = frame.current_row();
            let (col, row_offset) = write_input(&mut frame, view, term_cols)?;
            (col, start_row + row_offset)
        };

        // Mandatory rows so far + a blank spacer + the status line below =
        // budget floor. Anything left over is what the variable section
        // (picker/palette) can use; if the terminal is too short, it gets
        // nothing.
        let used = frame.rows() as usize + 2;
        let variable_budget = (term_rows as usize).saturating_sub(used);

        if let Some(picker) = view.picker {
            write_picker(&mut frame, picker, view.active_session_id, variable_budget)?;
        } else {
            write_palette(&mut frame, view, variable_budget)?;
        }

        // Blank row separating the input/picker area from the status line.
        frame.row()?;
        frame.row()?;
        write_status(&mut frame, view, term_cols)?;

        // Wipe whatever the previous render left on screen so a shrinking
        // prompt does not leave stale rows behind. Without the rule lines
        // above the input, no row above the cursor wraps on width shrink,
        // so the stored offset is still accurate.
        self.clear(out)?;

        // Allocate rows below current cursor: terminal scrolls if needed.
        let rows = frame.rows();
        if rows > 1 {
            for _ in 0..(rows - 1) {
                queue!(out, Print("\r\n"))?;
            }
            queue!(out, MoveUp(rows - 1))?;
        }
        queue!(out, MoveToColumn(0))?;

        out.write_all(frame.as_bytes())?;

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

#[derive(Default)]
struct Frame {
    buf: Vec<u8>,
    rows: u16,
}

impl Frame {
    fn row(&mut self) -> io::Result<()> {
        if self.rows > 0 {
            self.buf.write_all(b"\r\n")?;
        }
        self.rows += 1;
        Ok(())
    }

    fn current_row(&self) -> u16 {
        self.rows.saturating_sub(1)
    }

    fn rows(&self) -> u16 {
        self.rows
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf
    }
}

impl Write for Frame {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.buf.flush()
    }
}

// Returns (cursor_col, cursor_row_offset_from_first_input_row).
fn write_input(
    frame: &mut Frame,
    view: &View<'_>,
    term_cols: u16,
) -> io::Result<(u16, u16)> {
    let prefix = "> ";
    let prefix_cols: u16 = 2;

    queue!(frame, Print(prefix))?;

    let chars: Vec<char> = view.input.as_str().chars().collect();
    let cursor_idx = view.input.cursor_column() as usize;

    // Per-row capacity. The first row has the prefix in front; later rows
    // start at column 0. We reserve one column so the terminal's autowrap
    // never kicks in unexpectedly.
    let first_cap = term_cols
        .saturating_sub(prefix_cols)
        .saturating_sub(1)
        .max(1) as usize;
    let cont_cap = term_cols.saturating_sub(1).max(1) as usize;

    let mut cursor_col: u16 = prefix_cols;
    let mut cursor_row: u16 = 0;
    let mut idx = 0usize;
    let mut row: u16 = 0;
    let mut cap = first_cap;
    let mut col_base: u16 = prefix_cols;

    loop {
        let end = (idx + cap).min(chars.len());
        let chunk: String = chars[idx..end].iter().collect();
        queue!(frame, Print(chunk))?;

        if cursor_idx >= idx && cursor_idx <= end {
            cursor_row = row;
            cursor_col = col_base + (cursor_idx - idx) as u16;
        }

        idx = end;
        if idx >= chars.len() {
            break;
        }
        frame.row()?;
        row += 1;
        cap = cont_cap;
        col_base = 0;
    }

    Ok((cursor_col, cursor_row))
}

fn write_permission<W: Write>(out: &mut W, call: &ToolCall, term_cols: u16) -> io::Result<()> {
    let text = format!(
        " Run {}({})?  [y]es  [n]o ",
        call.name,
        print::preview(&call.input.to_string(), 60)
    );
    let max_cols = term_cols.saturating_sub(1) as usize;
    let truncated: String = text.chars().take(max_cols).collect();
    queue!(
        out,
        SetForegroundColor(theme::TOOL),
        SetAttribute(Attribute::Bold),
        Print(truncated),
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
    // Stay one column shy of the right edge so the terminal's autowrap
    // never kicks in and pushes the line onto an extra row.
    let max_cols = term_cols.saturating_sub(1) as usize;
    let left_cols = left.chars().count();
    let right_cols = right.chars().count();
    let text = if left_cols + right_cols >= max_cols {
        // Not enough room for both halves; drop the right side and
        // truncate the left to fit.
        left.chars().take(max_cols).collect::<String>()
    } else {
        let pad = max_cols - left_cols - right_cols;
        let mut t = left;
        t.push_str(&" ".repeat(pad));
        t.push_str(right);
        t
    };
    queue!(out, SetForegroundColor(theme::DIM), Print(text), ResetColor)
}

fn write_palette(frame: &mut Frame, view: &View<'_>, max_rows: usize) -> io::Result<()> {
    if max_rows == 0 {
        return Ok(());
    }
    let matches = commands::palette_matches(view.input.as_str());
    if matches.is_empty() {
        return Ok(());
    }
    let selected = view.palette_index.min(matches.len() - 1);
    let name_width = matches.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let visible = matches.len().min(max_rows);
    // Slide the window so the selected entry stays in view.
    let start = selected
        .saturating_sub(visible - 1)
        .min(matches.len() - visible);
    for offset in 0..visible {
        let i = start + offset;
        frame.row()?;
        write_palette_row(frame, &matches[i], name_width, i == selected)?;
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

fn write_picker(
    frame: &mut Frame,
    picker: &SessionPicker,
    active_id: Option<&str>,
    max_rows: usize,
) -> io::Result<()> {
    // Header + footer take 2 rows; need at least one row of items beyond that.
    if picker.is_empty() || max_rows < 3 {
        return Ok(());
    }
    let items_budget = (max_rows - 2).min(20);
    let selected = picker.selected_index();

    frame.row()?;
    queue!(
        frame,
        SetForegroundColor(theme::DIM),
        Print(" Sessions "),
        ResetColor,
    )?;

    for i in picker.visible_range(items_budget) {
        let Some(meta) = picker.session(i) else {
            continue;
        };
        let is_selected = Some(i) == selected;
        let is_active = Some(meta.id.as_str()) == active_id;
        let marker = if is_selected { ">" } else { " " };
        let active_marker = if is_active { "*" } else { " " };
        let title = if meta.title.is_empty() {
            "(untitled)"
        } else {
            meta.title.as_str()
        };
        let prefix = format!(" {marker}{active_marker} ");

        frame.row()?;
        if is_selected {
            queue!(
                frame,
                SetForegroundColor(theme::USER),
                SetAttribute(Attribute::Bold),
                Print(prefix),
                Print(title),
                SetAttribute(Attribute::NormalIntensity),
                ResetColor,
            )?;
        } else {
            queue!(frame, Print(prefix), Print(title))?;
        }
    }

    frame.row()?;
    queue!(
        frame,
        SetForegroundColor(theme::DIM),
        Print(" ↑/↓ navigate  ⏎ resume  esc cancel "),
        ResetColor,
    )?;
    Ok(())
}
