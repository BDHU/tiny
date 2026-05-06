mod input;
mod intro;
mod palette;
mod picker;
mod status;

use crate::tui::{state::State, theme};
use input::render_input;
use intro::render_intro;
use palette::render_palette;
use picker::render_picker;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use status::render_status;

pub(crate) fn ui(f: &mut ratatui::Frame, state: &State) {
    let [msg_area, input_area, status_area] = layout(f.area());

    if state.transcript.is_empty() && !state.turn.busy {
        render_intro(f, state, msg_area);
    } else {
        let lines = transcript_visible_lines(state, msg_area.height);
        f.render_widget(Paragraph::new(lines), msg_area);
    }

    render_input(f, state, input_area);
    render_status(f, state, status_area);
    render_palette(f, state, input_area);
    render_picker(f, state, input_area);
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

fn transcript_visible_lines(state: &State, height: u16) -> Vec<Line<'static>> {
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

fn bordered(style: Style) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style)
}
