use crate::tui::{state::State, theme};
use ratatui::{layout::Rect, style::Style, widgets::Paragraph};

pub(super) fn render_status(f: &mut ratatui::Frame, state: &State, area: Rect) {
    let msg_count = state.transcript.message_count();
    let queued = if state.queued == 0 {
        String::new()
    } else {
        format!(" · {} queued", state.queued)
    };
    let left = format!(
        " {} · {} msgs{} · {}",
        state.session.model, msg_count, queued, state.session.directory_label
    );
    let right = "⏎ send · /quit ";
    let left_cols = left.chars().count() as u16;
    let right_cols = right.chars().count() as u16;
    let pad = area
        .width
        .saturating_sub(left_cols)
        .saturating_sub(right_cols);
    let bar = format!("{left}{}{right}", " ".repeat(pad as usize));

    f.render_widget(
        Paragraph::new(bar).style(Style::default().fg(theme::DIM)),
        area,
    );
}
