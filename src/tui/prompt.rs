// Renders the prompt area (input bar + optional menu/picker/status) at the
// bottom of the terminal, anchored just below the scrollback. The terminal
// stays in raw mode without an alternate screen, so chat history lives in
// the terminal's own scrollback and we only manage these few rows.

use crate::tui::{
    commands::{self, Command},
    input::InputBuffer,
    modal::{Modal, ModalSlot},
    theme,
};
use crossterm::{
    cursor::{MoveToColumn, MoveUp},
    queue,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::{self, Write};
use std::time::Instant;

#[derive(Default)]
pub(crate) struct Prompt {
    cursor_offset_from_top: u16,
}

pub(crate) struct View<'a> {
    pub(crate) input: &'a InputBuffer,
    pub(crate) palette_index: usize,
    pub(crate) modal: Option<&'a dyn Modal>,
    pub(crate) turn_started_at: Option<Instant>,
    pub(crate) queued: usize,
    pub(crate) model: &'a str,
    pub(crate) directory_label: &'a str,
    pub(crate) message_count: usize,
    pub(crate) terminal_size: (u16, u16),
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

        let mut frame = Frame::default();

        frame.row()?;

        let input_modal = view
            .modal
            .filter(|m| matches!(m.slot(), ModalSlot::Input));
        let panel_modal = view
            .modal
            .filter(|m| matches!(m.slot(), ModalSlot::Panel));

        let (input_col, input_row) = if let Some(modal) = input_modal {
            let row = frame.row()?;
            modal.render(&mut frame, term_cols, 1)?;
            (0, row)
        } else {
            let start_row = frame.row()?;
            let (col, row_offset) = write_input(&mut frame, view, term_cols)?;
            (col, start_row + row_offset)
        };

        // Reserve the final row for status. Suggestions and pickers take
        // whatever remains between the input and that bottom status row.
        let variable_budget = (term_rows as usize).saturating_sub(frame.rows() as usize + 2);

        if let Some(modal) = panel_modal {
            modal.render(&mut frame, term_cols, variable_budget)?;
        } else {
            write_palette(&mut frame, view, variable_budget, term_cols)?;
        }

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
pub(crate) struct Frame {
    buf: Vec<u8>,
    rows: u16,
}

impl Frame {
    pub(crate) fn row(&mut self) -> io::Result<u16> {
        if self.rows > 0 {
            self.buf.write_all(b"\r\n")?;
        }
        self.rows += 1;
        Ok(self.rows - 1)
    }

    pub(crate) fn rows(&self) -> u16 {
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
fn write_input(frame: &mut Frame, view: &View<'_>, term_cols: u16) -> io::Result<(u16, u16)> {
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

fn write_status<W: Write>(out: &mut W, view: &View<'_>, term_cols: u16) -> io::Result<()> {
    let started = view.turn_started_at;
    let elapsed_ticks = started.map(|t| t.elapsed().as_millis() / 80).unwrap_or(0) as usize;
    let busy = if let Some(started) = started {
        let queued = if view.queued == 0 {
            String::new()
        } else {
            format!(" · {} queued", view.queued)
        };
        format!(
            " · {} {}s{}",
            theme::SPINNER[elapsed_ticks % theme::SPINNER.len()],
            started.elapsed().as_secs(),
            queued
        )
    } else {
        String::new()
    };
    let text = format!(
        " {} · {} msgs · {}{}",
        view.model, view.message_count, view.directory_label, busy
    );
    queue!(
        out,
        SetForegroundColor(theme::DIM),
        Print(fit_line(&text, term_cols)),
        ResetColor,
    )
}

fn write_palette(
    frame: &mut Frame,
    view: &View<'_>,
    max_rows: usize,
    term_cols: u16,
) -> io::Result<()> {
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
    let start = selected
        .saturating_sub(visible - 1)
        .min(matches.len() - visible);
    for offset in 0..visible {
        let i = start + offset;
        frame.row()?;
        write_palette_row(frame, matches[i], name_width, i == selected, term_cols)?;
    }
    Ok(())
}

fn write_palette_row<W: Write>(
    out: &mut W,
    cmd: &Command,
    name_width: usize,
    selected: bool,
    term_cols: u16,
) -> io::Result<()> {
    let pad = " ".repeat(name_width.saturating_sub(cmd.name.len()));
    let marker = if selected { " > " } else { "   " };
    let text = format!("{marker}/{}{}  {}", cmd.name, pad, cmd.help);
    write_choice_line(out, &text, selected, term_cols)
}

pub(crate) fn write_choice_line<W: Write>(
    out: &mut W,
    text: &str,
    selected: bool,
    term_cols: u16,
) -> io::Result<()> {
    let color = if selected { theme::USER } else { theme::DIM };
    queue!(out, SetForegroundColor(color))?;
    if selected {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    queue!(out, Print(fit_line(text, term_cols)))?;
    if selected {
        queue!(out, SetAttribute(Attribute::NormalIntensity))?;
    }
    queue!(out, ResetColor)
}

pub(crate) fn fit_line(text: &str, term_cols: u16) -> String {
    let max_cols = term_cols.saturating_sub(1) as usize;
    text.chars().take(max_cols).collect()
}
