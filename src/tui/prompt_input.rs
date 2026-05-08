use crate::tui::{frame::Frame, input::InputBuffer};
use crossterm::{queue, style::Print};
use std::io;

pub(crate) struct InputCursor {
    pub(crate) col: u16,
    pub(crate) row_offset: u16,
}

pub(crate) fn write_input(
    frame: &mut Frame,
    input: &InputBuffer,
    term_cols: u16,
) -> io::Result<InputCursor> {
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

    let mut cursor = InputCursor {
        col: prefix_cols,
        row_offset: 0,
    };
    let mut idx = 0usize;
    let mut row: u16 = 0;
    let mut cap = first_cap;
    let mut col_base: u16 = prefix_cols;

    loop {
        let end = (idx + cap).min(chars.len());
        let chunk: String = chars[idx..end].iter().collect();
        queue!(frame, Print(chunk))?;

        if cursor_idx >= idx && cursor_idx <= end {
            cursor = InputCursor {
                row_offset: row,
                col: col_base + (cursor_idx - idx) as u16,
            };
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

    Ok(cursor)
}
