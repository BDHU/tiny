use crate::tui::{state::State, theme, transcript::preview};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::Paragraph,
};

pub(super) fn render_input(f: &mut ratatui::Frame, state: &State, area: Rect) {
    if let Some(pending) = &state.pending {
        let prompt = format!(
            " Run {}({})?  [y]es  [n]o ",
            pending.call.name,
            preview(&pending.call.input.to_string(), 60)
        );
        f.render_widget(
            Paragraph::new(prompt)
                .style(
                    Style::default()
                        .fg(theme::TOOL)
                        .add_modifier(Modifier::BOLD),
                )
                .block(super::bordered(Style::default().fg(theme::TOOL))),
            area,
        );
        return;
    }

    let border_style = if state.turn.busy {
        Style::default().fg(theme::DIM)
    } else {
        Style::default().fg(theme::USER)
    };
    let prefix = "> ";
    let prefix_cols = prefix.chars().count() as u16;
    let inner_width = area.width.saturating_sub(2);
    let cursor_col = prefix_cols + state.input.cursor_column();
    let scroll_x = cursor_col.saturating_sub(inner_width.saturating_sub(1));
    let text = format!("{prefix}{}", state.input.as_str());

    f.render_widget(
        Paragraph::new(text)
            .scroll((0, scroll_x))
            .block(super::bordered(border_style)),
        area,
    );
    f.set_cursor_position((area.x + 1 + cursor_col - scroll_x, area.y + 1));
}
