use crate::tui::{
    commands,
    state::State,
    theme,
    transcript::{ellipsize, preview},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
};

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

fn render_intro(f: &mut ratatui::Frame, state: &State, area: Rect) {
    let lines = intro_lines(state);
    let content_width = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
    let width = (content_width + 4).min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(1));
    if width == 0 || height == 0 {
        return;
    }
    let box_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width,
        height,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::DIM))
        .padding(Padding::horizontal(1));
    f.render_widget(Paragraph::new(lines).block(block), box_area);
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

fn render_picker(f: &mut ratatui::Frame, state: &State, input_area: Rect) {
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

    // Layout inside borders: " >* " + title.  List is sorted recent-first,
    // so position carries the freshness signal — no timestamp column.
    const PREFIX_COLS: usize = 4;
    let desired_title: usize = picker
        .sessions
        .iter()
        .map(|s| s.title.chars().count().max("(untitled)".len()))
        .max()
        .unwrap_or(11);
    // 2 borders + prefix + title + right padding (1)
    let desired_width = (2 + PREFIX_COLS + desired_title + 1) as u16;
    let width = desired_width.min(input_area.width).max(20);
    let title_max = (width as usize).saturating_sub(2 + PREFIX_COLS + 1);
    let height = row_count + chrome;
    let y = input_area.y.saturating_sub(height);
    let area = Rect {
        x: input_area.x,
        y,
        width,
        height,
    };

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
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::USER)),
        ),
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
        "(untitled)".to_string()
    } else {
        meta.title.clone()
    };
    let title = ellipsize(&title, title_max);
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

fn render_palette(f: &mut ratatui::Frame, state: &State, input_area: Rect) {
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
    let area = Rect {
        x: input_area.x,
        y,
        width,
        height,
    };

    let lines: Vec<Line<'static>> = matches
        .iter()
        .enumerate()
        .map(|(i, cmd)| palette_line(cmd.name, cmd.help, name_width, help_max, i == selected))
        .collect();

    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::DIM)),
        ),
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
