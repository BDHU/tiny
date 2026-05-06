use crate::tui::{commands, state::State, theme, transcript::ellipsize};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

pub(super) fn render_palette(f: &mut ratatui::Frame, state: &State, input_area: Rect) {
    if state.pending.is_some() || state.picker.is_some() {
        return;
    }
    let matches = commands::palette_matches(state.input.as_str());
    if matches.is_empty() {
        return;
    }

    let selected = state.palette_index.min(matches.len() - 1);
    let name_width = matches.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let help_width = matches.iter().map(|c| c.help.len()).max().unwrap_or(0);
    // Inside borders: "   /name<pad>  help".  3 marker + 1 slash + name + 2 gap + help.
    let prefix_cols = 3 + 1 + name_width + 2;
    let desired_width = (2 + prefix_cols + help_width) as u16;
    let width = desired_width.min(input_area.width).max(20);
    let help_max = (width as usize).saturating_sub(2 + prefix_cols);
    let height = (matches.len() as u16 + 2).min(input_area.y.max(1));
    let y = input_area.y.saturating_sub(height);
    let area = Rect::new(input_area.x, y, width, height);

    let lines: Vec<Line<'static>> = matches
        .iter()
        .enumerate()
        .map(|(i, cmd)| palette_line(cmd.name, cmd.help, name_width, help_max, i == selected))
        .collect();

    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(super::bordered(Style::default().fg(theme::DIM))),
        area,
    );
}

fn palette_line(
    name: &str,
    help: &str,
    name_width: usize,
    help_max: usize,
    selected: bool,
) -> Line<'static> {
    let name_pad = " ".repeat(name_width.saturating_sub(name.len()));
    let marker_style = if selected {
        Style::default()
            .fg(theme::USER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::DIM)
    };
    let name_style = if selected {
        Style::default()
            .fg(theme::USER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(if selected { " > " } else { "   " }, marker_style),
        Span::styled(format!("/{name}"), name_style),
        Span::raw(name_pad),
        Span::raw("  "),
        Span::styled(ellipsize(help, help_max), Style::default().fg(theme::DIM)),
    ])
}
