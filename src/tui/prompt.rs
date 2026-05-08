// Renders the prompt area (input bar + optional menu/picker/status) at the
// bottom of the terminal, anchored just below the scrollback. The terminal
// stays in raw mode without an alternate screen, so chat history lives in
// the terminal's own scrollback and we only manage these few rows.

use crate::tui::{
    input::InputBuffer,
    state::AppState,
    surface::{Line, RenderCtx, Style, Surface},
    theme,
};
use crossterm::{
    cursor::{MoveToColumn, MoveUp},
    queue,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::{self, Write};

#[derive(Default)]
pub(crate) struct Prompt {
    cursor_offset_from_top: u16,
}

pub(crate) struct PromptFrame {
    frame: Frame,
    input_col: u16,
    input_row: u16,
}

impl PromptFrame {
    pub(crate) fn rows(&self) -> u16 {
        self.frame.rows()
    }

    pub(crate) fn input_cursor(&self) -> (u16, u16) {
        (self.input_col, self.input_row)
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.frame.as_bytes()
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

    pub(crate) fn render<W: Write>(
        &mut self,
        out: &mut W,
        state: &AppState,
        terminal_size: (u16, u16),
    ) -> io::Result<()> {
        let prompt_frame = build_frame(state, terminal_size)?;

        // Wipe whatever the previous render left on screen so a shrinking
        // prompt does not leave stale rows behind. Without the rule lines
        // above the input, no row above the cursor wraps on width shrink,
        // so the stored offset is still accurate.
        self.clear(out)?;

        // Allocate rows below current cursor: terminal scrolls if needed.
        let rows = prompt_frame.rows();
        if rows > 1 {
            for _ in 0..(rows - 1) {
                queue!(out, Print("\r\n"))?;
            }
            queue!(out, MoveUp(rows - 1))?;
        }
        queue!(out, MoveToColumn(0))?;

        out.write_all(prompt_frame.as_bytes())?;

        // Cursor was left on the last row, move up to the input row.
        let (input_col, input_row) = prompt_frame.input_cursor();
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

pub(crate) fn build_frame(state: &AppState, terminal_size: (u16, u16)) -> io::Result<PromptFrame> {
    let (term_cols, term_rows) = terminal_size;
    let mut frame = Frame::default();

    frame.row()?;

    let modal = state.modal.as_deref().filter(|m| m.is_visible(state));
    let overlay = state.overlay.as_deref().filter(|m| m.is_visible(state));
    // Takeover panel modal wins; otherwise the overlay (palette) takes the
    // panel. Draft input is always rendered by the prompt itself.
    let panel_modal = modal.or_else(|| overlay);

    let start_row = frame.row()?;
    let (input_col, row_offset) = write_input(&mut frame, &state.input, term_cols)?;
    let input_row = start_row + row_offset;

    // Reserve the final row for status. Suggestions and pickers take whatever
    // remains between the input and that bottom status row.
    let variable_budget = (term_rows as usize).saturating_sub(frame.rows() as usize + 2);

    if let Some(modal) = panel_modal {
        let surface = modal.render(RenderCtx {
            cols: term_cols,
            max_rows: variable_budget,
            state,
        });
        frame.render_surface(&surface, term_cols)?;
    }

    frame.row()?;
    frame.row()?;
    write_status(&mut frame, state, term_cols)?;

    Ok(PromptFrame {
        frame,
        input_col,
        input_row,
    })
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

    pub(crate) fn render_surface(&mut self, surface: &Surface, term_cols: u16) -> io::Result<()> {
        for line in &surface.lines {
            self.row()?;
            self.write_line(line, term_cols)?;
        }
        Ok(())
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    fn write_line(&mut self, line: &Line, term_cols: u16) -> io::Result<()> {
        let mut remaining = term_cols.saturating_sub(1) as usize;
        for span in &line.spans {
            if remaining == 0 {
                break;
            }
            let text: String = span.text.chars().take(remaining).collect();
            let cols = text.chars().count();
            if cols == 0 {
                continue;
            }
            write_style(self, span.style)?;
            queue!(self, Print(text))?;
            reset_style(self, span.style)?;
            remaining = remaining.saturating_sub(cols);
        }
        Ok(())
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
fn write_input(frame: &mut Frame, input: &InputBuffer, term_cols: u16) -> io::Result<(u16, u16)> {
    let prefix = "> ";
    let prefix_cols: u16 = 2;

    queue!(frame, Print(prefix))?;

    let chars: Vec<char> = input.as_str().chars().collect();
    let cursor_idx = input.cursor_column() as usize;

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

fn write_status<W: Write>(out: &mut W, state: &AppState, term_cols: u16) -> io::Result<()> {
    let session = state.session.as_ref();
    let model = session.map(|s| s.model.as_str()).unwrap_or("unknown");
    let message_count = session.map(|s| s.message_count).unwrap_or(0);
    // Hide the queued counter while a takeover modal is up; the user's mental
    // model is "current turn paused on prompt", and the queue isn't growing.
    let queued = if state.modal.is_none() {
        state.turn.as_ref().map(|t| t.queued).unwrap_or(0)
    } else {
        0
    };
    let started = state.turn.as_ref().map(|t| t.started_at);
    let elapsed_ticks = started.map(|t| t.elapsed().as_millis() / 80).unwrap_or(0) as usize;
    let busy = if let Some(started) = started {
        let queued_str = if queued == 0 {
            String::new()
        } else {
            format!(" · {queued} queued")
        };
        format!(
            " · {} {}s{}",
            theme::SPINNER[elapsed_ticks % theme::SPINNER.len()],
            started.elapsed().as_secs(),
            queued_str
        )
    } else {
        String::new()
    };
    let text = format!(
        " {} · {} msgs · {}{}",
        model, message_count, state.directory_label, busy
    );
    queue!(
        out,
        SetForegroundColor(theme::DIM),
        Print(fit_line(&text, term_cols)),
        ResetColor,
    )
}

fn write_style<W: Write>(out: &mut W, style: Style) -> io::Result<()> {
    if let Some(color) = style.fg {
        queue!(out, SetForegroundColor(color))?;
    }
    if style.bold {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    Ok(())
}

fn reset_style<W: Write>(out: &mut W, style: Style) -> io::Result<()> {
    if style.bold {
        queue!(out, SetAttribute(Attribute::NormalIntensity))?;
    }
    if style.fg.is_some() {
        queue!(out, ResetColor)?;
    }
    Ok(())
}

pub(crate) fn fit_line(text: &str, term_cols: u16) -> String {
    let max_cols = term_cols.saturating_sub(1) as usize;
    text.chars().take(max_cols).collect()
}

#[cfg(test)]
mod tests {
    use super::build_frame;
    use crate::tui::{permission::PermissionPromptModal, state::AppState};
    use serde_json::json;
    use tiny::ToolCall;

    #[test]
    fn permission_prompt_keeps_draft_input_visible() {
        let mut state = AppState::new();
        state.input.insert_str("draft");
        state.modal = Some(Box::new(PermissionPromptModal::new(
            0,
            ToolCall {
                id: "call-1".into(),
                name: "bash".into(),
                input: json!({"command": "git status"}),
            },
        )));

        let frame = build_frame(&state, (40, 10)).expect("frame");
        let lines = visible_lines(frame.as_bytes());
        let (cursor_col, cursor_row) = frame.input_cursor();

        assert!(lines.iter().any(|line| line == "> draft"));
        assert!(lines.iter().any(|line| line.contains("allow bash(")));
        assert_eq!(lines[cursor_row as usize], "> draft");
        assert_eq!(cursor_col, 7);
    }

    #[test]
    fn prompt_frame_lines_fit_narrow_terminal() {
        let mut state = AppState::new();
        state.input.insert_str("/");

        let frame = build_frame(&state, (20, 8)).expect("frame");
        let lines = visible_lines(frame.as_bytes());

        assert_eq!(frame.rows() as usize, lines.len());
        for line in lines {
            assert!(
                line.chars().count() <= 19,
                "line exceeded reserved width: {line:?}"
            );
        }
    }

    fn visible_lines(bytes: &[u8]) -> Vec<String> {
        let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
        text.split('\n').map(strip_ansi).collect()
    }

    fn strip_ansi(input: &str) -> String {
        let mut out = String::new();
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' && chars.peek() == Some(&'[') {
                chars.next();
                for code in chars.by_ref() {
                    if code.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }
}
