use crate::tui::surface::{Line, Style, Surface};
use crossterm::{
    queue,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{self, Write};

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

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub(crate) fn render_surface(&mut self, surface: &Surface, term_cols: u16) -> io::Result<()> {
        for line in surface.lines() {
            self.row()?;
            self.write_line(line, term_cols)?;
        }
        Ok(())
    }

    fn write_line(&mut self, line: &Line, term_cols: u16) -> io::Result<()> {
        let mut remaining = term_cols.saturating_sub(1) as usize;
        for span in line.spans() {
            if remaining == 0 {
                break;
            }
            let text: String = span.text().chars().take(remaining).collect();
            let cols = text.chars().count();
            if cols == 0 {
                continue;
            }
            write_style(self, span.style())?;
            queue!(self, Print(text))?;
            reset_style(self, span.style())?;
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

pub(crate) fn fit_line(text: &str, term_cols: u16) -> String {
    let max_cols = term_cols.saturating_sub(1) as usize;
    text.chars().take(max_cols).collect()
}

fn write_style<W: Write>(out: &mut W, style: Style) -> io::Result<()> {
    if let Some(color) = style.fg_color() {
        queue!(out, SetForegroundColor(color))?;
    }
    if style.is_bold() {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    Ok(())
}

fn reset_style<W: Write>(out: &mut W, style: Style) -> io::Result<()> {
    if style.is_bold() {
        queue!(out, SetAttribute(Attribute::NormalIntensity))?;
    }
    if style.fg_color().is_some() {
        queue!(out, ResetColor)?;
    }
    Ok(())
}
