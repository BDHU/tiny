use crate::tui::{state::AppState, theme};
use crossterm::style::Color;

#[derive(Clone, Copy, Default)]
pub(crate) struct Style {
    fg: Option<Color>,
    bold: bool,
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

    pub(crate) fn fg_color(&self) -> Option<Color> {
        self.fg
    }

    pub(crate) fn is_bold(&self) -> bool {
        self.bold
    }
}

pub(crate) struct Span {
    text: String,
    style: Style,
}

impl Span {
    pub(crate) fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn style(&self) -> Style {
        self.style
    }
}

#[derive(Default)]
pub(crate) struct Line {
    spans: Vec<Span>,
}

impl Line {
    pub(crate) fn styled(text: impl Into<String>, style: Style) -> Self {
        Self {
            spans: vec![Span::new(text, style)],
        }
    }

    pub(crate) fn spans(&self) -> &[Span] {
        &self.spans
    }
}

#[derive(Default)]
pub(crate) struct Surface {
    lines: Vec<Line>,
}

impl Surface {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn line(mut self, line: Line) -> Self {
        self.lines.push(line);
        self
    }

    pub(crate) fn push_line(&mut self, line: Line) {
        self.lines.push(line);
    }

    pub(crate) fn lines(&self) -> &[Line] {
        &self.lines
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
