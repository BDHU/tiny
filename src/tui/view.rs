use crate::tui::{state::State, theme, transcript::preview};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

pub(crate) fn ui(f: &mut ratatui::Frame, state: &State) {
    let [msg_area, input_area, status_area] = layout(f.area());
    let lines = transcript_visible_lines(state, msg_area.height);

    f.render_widget(Paragraph::new(lines), msg_area);

    render_input(f, state, input_area);
    render_status(f, state, status_area);
}

pub(crate) fn message_area(area: Rect) -> Rect {
    layout(area)[0]
}

fn layout(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(area)
}

fn intro_lines(state: &State) -> Vec<Line<'static>> {
    let dim = Style::default().fg(theme::DIM);
    vec![
        Line::default(),
        Line::from(vec![
            Span::raw(theme::GUTTER),
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
            Span::raw(theme::GUTTER),
            Span::styled("model:     ", dim),
            Span::raw(state.session.model.clone()),
        ]),
        Line::from(vec![
            Span::raw(theme::GUTTER),
            Span::styled("directory: ", dim),
            Span::raw(state.session.directory.clone()),
        ]),
        Line::default(),
        Line::from(vec![
            Span::raw(theme::GUTTER),
            Span::styled("Type a message and press ⏎ to begin.", dim),
        ]),
    ]
}

fn transcript_visible_lines(state: &State, height: u16) -> Vec<Line<'static>> {
    if state.transcript.is_empty() && !state.turn.busy {
        return intro_lines(state);
    }

    let mut lines = Vec::new();
    let start = state.scroll.offset as usize;
    let end = start.saturating_add(height as usize);
    let transcript_height = state.transcript.height() as usize;

    for index in start..end {
        if let Some(line) = state.transcript.line_at(index) {
            lines.push(line);
            continue;
        }

        if state.turn.busy {
            match index.saturating_sub(transcript_height) {
                0 => lines.push(Line::default()),
                1 => lines.push(spinner_line(state)),
                _ => {}
            }
        }
    }

    lines
}

fn spinner_line(state: &State) -> Line<'static> {
    let elapsed = state
        .turn
        .started_at
        .map(|started| started.elapsed().as_secs())
        .unwrap_or(0);
    Line::from(vec![
        Span::raw(theme::GUTTER),
        Span::styled(
            theme::SPINNER[state.tick % theme::SPINNER.len()].to_string(),
            Style::default().fg(theme::TOOL),
        ),
        Span::styled(
            format!(" Thinking... ({elapsed}s)"),
            Style::default().fg(theme::DIM),
        ),
    ])
}

fn render_input(f: &mut ratatui::Frame, state: &State, area: Rect) {
    if let Some(pending) = &state.pending {
        let prompt = format!(
            " Allow {}({})?  [y]es  [n]o ",
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
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(theme::TOOL)),
                ),
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
        Paragraph::new(text).scroll((0, scroll_x)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style),
        ),
        area,
    );
    f.set_cursor_position((area.x + 1 + cursor_col - scroll_x, area.y + 1));
}

fn render_status(f: &mut ratatui::Frame, state: &State, area: Rect) {
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
    let right = "⏎ send · ^D quit ";
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
