use crate::tui::{state::AppState, theme};
use crossterm::style::Color;

#[derive(Clone, Copy, Default)]
pub(crate) struct Style {
    pub(crate) fg: Option<Color>,
    pub(crate) bold: bool,
}

impl Style {
    pub(crate) fn fg(fg: Color) -> Self {
        Self {
            fg: Some(fg),
            bold: false,
        }
    }

    pub(crate) fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

pub(crate) struct Span {
    pub(crate) text: String,
    pub(crate) style: Style,
}

impl Span {
    pub(crate) fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

#[derive(Default)]
pub(crate) struct Line {
    pub(crate) spans: Vec<Span>,
}

impl Line {
    pub(crate) fn styled(text: impl Into<String>, style: Style) -> Self {
        Self {
            spans: vec![Span::new(text, style)],
        }
    }
}

#[derive(Default)]
pub(crate) struct Surface {
    pub(crate) lines: Vec<Line>,
}

impl Surface {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn line(mut self, line: Line) -> Self {
        self.lines.push(line);
        self
    }
}

pub(crate) struct RenderCtx<'a> {
    pub(crate) cols: u16,
    pub(crate) max_rows: usize,
    pub(crate) state: &'a AppState,
}

pub(crate) fn choice_line(text: impl Into<String>, selected: bool) -> Line {
    let style = if selected {
        Style::fg(theme::USER).bold()
    } else {
        Style::fg(theme::DIM)
    };
    Line::styled(text, style)
}
