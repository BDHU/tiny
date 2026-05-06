use crate::tui::{state::State, theme, transcript::ellipsize};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

pub(super) fn render_picker(f: &mut ratatui::Frame, state: &State, input_area: Rect) {
    let Some(picker) = &state.picker else {
        return;
    };
    if picker.sessions.is_empty() || input_area.width < 4 || input_area.y < 4 {
        return;
    }

    let active = state.session.id.as_deref();
    let selected = picker.selected.min(picker.sessions.len() - 1);

    // Total chrome around rows: top border + footer + bottom border = 3.
    let chrome: u16 = 3;
    let max_rows = input_area.y.saturating_sub(chrome).min(20);
    let row_count = (picker.sessions.len() as u16).min(max_rows).max(1);

    // Scroll window so the selected row stays visible.
    let window = row_count as usize;
    let start = if selected >= window {
        selected + 1 - window
    } else {
        0
    };
    let end = (start + window).min(picker.sessions.len());

    const PREFIX_COLS: usize = 4;
    let desired_title: usize = picker
        .sessions
        .iter()
        .map(|s| s.title.chars().count().max("(untitled)".len()))
        .max()
        .unwrap_or(11);
    let desired_width = (2 + PREFIX_COLS + desired_title + 1) as u16;
    let width = desired_width.min(input_area.width).max(20);
    let title_max = (width as usize).saturating_sub(2 + PREFIX_COLS + 1);
    let height = row_count + chrome;
    let y = input_area.y.saturating_sub(height);
    let area = Rect::new(input_area.x, y, width, height);

    let dim = Style::default().fg(theme::DIM);

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(row_count as usize + 1);
    for i in start..end {
        let meta = &picker.sessions[i];
        let is_selected = i == selected;
        let is_active = Some(meta.id.as_str()) == active;
        lines.push(picker_line(meta, is_selected, is_active, title_max));
    }
    lines.push(Line::from(vec![Span::styled(
        " ↑/↓ navigate  ⏎ resume  esc cancel",
        dim,
    )]));

    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(super::bordered(Style::default().fg(theme::USER))),
        area,
    );
}

fn picker_line(
    meta: &tiny::SessionMeta,
    selected: bool,
    active: bool,
    title_max: usize,
) -> Line<'static> {
    let marker_text = if selected { ">" } else { " " };
    let active_text = if active { "*" } else { " " };
    let title = if meta.title.is_empty() {
        "(untitled)"
    } else {
        meta.title.as_str()
    };
    let title = ellipsize(title, title_max);
    let row_style = if selected {
        Style::default()
            .fg(theme::USER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(format!(" {marker_text}{active_text} "), row_style),
        Span::styled(title, row_style),
    ])
}
