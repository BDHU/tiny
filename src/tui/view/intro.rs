use crate::tui::{state::State, theme};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Padding, Paragraph},
};

pub(super) fn render_intro(f: &mut ratatui::Frame, state: &State, area: Rect) {
    let lines = intro_lines(state);
    let content_width = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
    let width = (content_width + 4).min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(1));
    if width == 0 || height == 0 {
        return;
    }

    let box_area = Rect::new(area.x + 1, area.y + 1, width, height);
    let block = super::bordered(Style::default().fg(theme::DIM)).padding(Padding::horizontal(1));
    f.render_widget(Paragraph::new(lines).block(block), box_area);
}

fn intro_lines(state: &State) -> Vec<Line<'static>> {
    let dim = Style::default().fg(theme::DIM);
    vec![
        Line::from(vec![
            Span::styled(
                ">_ ",
                Style::default()
                    .fg(theme::USER)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("Tiny (v{})", env!("CARGO_PKG_VERSION")),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::default(),
        Line::from(vec![
            Span::styled("model:     ", dim),
            Span::raw(state.session.model.clone()),
        ]),
        Line::from(vec![
            Span::styled("directory: ", dim),
            Span::raw(state.session.directory.clone()),
        ]),
        Line::default(),
        Line::from(Span::styled("Type a message and press ⏎ to begin.", dim)),
    ]
}
