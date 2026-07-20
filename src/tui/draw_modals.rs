//! Drawing the confirm + brain-input modals.

use super::*;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Paragraph,
    },
};

pub(crate) fn draw_confirm(f: &mut Frame, state: &ConfirmState, area: Rect) {
    // 60 wide leaves room for moderately long task names; the label is
    // truncated below if it still overflows. Height is 8 to fit the
    // new task-label row between the prompt and the buttons.
    let modal = centered_rect(60.min(area.width), 8.min(area.height), area);
    f.render_widget(Clear, modal);

    // Accent follows the confirmation's intent: green for constructive
    // actions (mark-complete, generate agenda, triage), red for destructive
    // ones (remove). Distinct from the cyan brain-input and purple palette.
    let accent = state.intent.accent();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                state.title.clone(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // spacer
            Constraint::Length(1), // prompt
            Constraint::Length(1), // task label (dim)
            Constraint::Length(1), // spacer
            Constraint::Length(1), // yes / no buttons
            Constraint::Min(0),    // spacer
            Constraint::Length(1), // footer hint
        ])
        .split(inner);

    let prompt = Paragraph::new(Line::from(Span::styled(
        state.prompt.clone(),
        Style::default().fg(Color::Rgb(192, 202, 245)),
    )))
    .alignment(Alignment::Center);
    f.render_widget(prompt, chunks[1]);

    // Trim the label to fit the inner width with a small margin.
    let max_label_chars = usize::from(inner.width).saturating_sub(4).max(8);
    let label_text = crate::tasks::render::truncate(&state.task_label, max_label_chars);
    let label = Paragraph::new(Line::from(Span::styled(
        label_text,
        Style::default().fg(Color::Rgb(122, 134, 173)),
    )))
    .alignment(Alignment::Center);
    f.render_widget(label, chunks[2]);

    let button_style = |focused: bool| {
        if focused {
            Style::default()
                .fg(Color::Rgb(36, 40, 59))
                .bg(accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(122, 134, 173))
        }
    };
    let mut button_spans = Vec::new();
    for (i, &choice) in state.choices().iter().enumerate() {
        if i > 0 {
            button_spans.push(Span::raw("    "));
        }
        let label = match choice {
            ConfirmChoice::Yes => " Yes ",
            ConfirmChoice::No => " No ",
            ConfirmChoice::Skip => " Skip ",
        };
        button_spans.push(Span::styled(label, button_style(state.focus == choice)));
    }
    let buttons = Paragraph::new(Line::from(button_spans)).alignment(Alignment::Center);
    f.render_widget(buttons, chunks[4]);

    let key_style = Style::default()
        .fg(Color::Rgb(192, 202, 245))
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::Rgb(122, 134, 173));
    let mut footer_spans = vec![
        Span::styled("y", key_style),
        Span::styled(" yes  ", dim),
        Span::styled("n", key_style),
        Span::styled(" no  ", dim),
    ];
    if state.has_skip() {
        footer_spans.push(Span::styled("s", key_style));
        footer_spans.push(Span::styled(" skip  ", dim));
    }
    footer_spans.extend([
        Span::styled("←/→", key_style),
        Span::styled(" focus  ", dim),
        Span::styled("Enter", key_style),
        Span::styled(" confirm  ", dim),
        Span::styled("Esc", key_style),
        Span::styled(" cancel", dim),
    ]);
    let footer = Paragraph::new(Line::from(footer_spans)).alignment(Alignment::Center);
    f.render_widget(footer, chunks[6]);
}

pub(crate) fn draw_link_picker(f: &mut Frame, state: &LinkPickerState, area: Rect) {
    let links = state.links();
    // border(2) + title row is in the border + list + footer(1) + a spacer.
    let list_h = u16::try_from(links.len()).unwrap_or(u16::MAX);
    let height = list_h.saturating_add(4).max(7).min(area.height);
    let modal = centered_rect(72.min(area.width), height, area);
    f.render_widget(Clear, modal);

    // Cyan accent — opening a link is a benign navigation action, distinct
    // from the destructive pink confirm and the purple command palette.
    let accent = Color::Rgb(125, 207, 255);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                state.title(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // prompt
            Constraint::Length(1), // separator
            Constraint::Min(1),    // list
            Constraint::Length(1), // footer
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Which link do you want to open?",
                Style::default().fg(Color::Rgb(192, 202, 245)),
            ),
        ])),
        chunks[0],
    );

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(usize::from(chunks[1].width)),
            Style::default().fg(Color::Rgb(78, 92, 122)),
        ))),
        chunks[1],
    );

    let active = Style::default()
        .fg(Color::Rgb(255, 199, 119))
        .add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(Color::Rgb(192, 202, 245));
    let dim = Style::default().fg(Color::Rgb(122, 134, 173));
    let url_budget = usize::from(chunks[2].width).saturating_sub(20).max(16);
    let rows: Vec<Line> = links
        .iter()
        .enumerate()
        .map(|(i, link)| {
            let (prefix, label_style) = if i == state.selected() {
                (" ▎ ", active)
            } else {
                ("   ", inactive)
            };
            let mut spans = vec![
                Span::styled(prefix, label_style),
                Span::styled(format!("{}. ", i + 1), label_style),
                Span::styled(link.label.clone(), label_style),
            ];
            // For the Linear row the label is "Linear AVA-123"; surface the
            // URL dimmed alongside it. Notes rows already are the URL.
            if link.label != link.url {
                spans.push(Span::styled(
                    format!("  {}", crate::tasks::render::truncate(&link.url, url_budget)),
                    dim,
                ));
            }
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(rows), chunks[2]);

    let key_style = Style::default()
        .fg(Color::Rgb(192, 202, 245))
        .add_modifier(Modifier::BOLD);
    let footer = Paragraph::new(Line::from(vec![
        Span::raw(" "),
        Span::styled("↑↓", key_style),
        Span::styled(" navigate  ", dim),
        Span::styled("#", key_style),
        Span::styled(" open  ", dim),
        Span::styled("Enter", key_style),
        Span::styled(" open  ", dim),
        Span::styled("Esc", key_style),
        Span::styled(" close", dim),
    ]));
    f.render_widget(footer, chunks[3]);
}

/// Soft-wrap `text` into display rows no wider than `width`, honoring
/// explicit `\n` as hard line breaks and preferring to break between
/// words. A word longer than `width` is hard-split at the boundary.
/// Every character is assigned to exactly one row, in order, so the
/// cursor (always at the end of the buffer in this modal) maps to the end
/// of the last row. Always returns at least one (possibly empty) row.
pub(crate) fn wrap_input(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        let mut start = 0;
        loop {
            if chars.len() - start <= width {
                rows.push(chars[start..].iter().collect());
                break;
            }
            // Prefer breaking after the last space inside the window;
            // otherwise hard-break at the width boundary.
            let window_end = start + width;
            let mut end = window_end;
            for i in (start..window_end).rev() {
                if chars[i] == ' ' {
                    end = i + 1;
                    break;
                }
            }
            rows.push(chars[start..end].iter().collect());
            start = end;
        }
    }
    rows
}

/// Horizontal room taken by the `" > "` prompt / continuation indent.
const BRAIN_INPUT_MARGIN: usize = 3;

pub(crate) fn draw_brain_input(f: &mut Frame, state: &BrainInputState, area: Rect) {
    let accent = Color::Rgb(125, 207, 255);

    // Wrap the buffer to the text column so the input grows downward and
    // reads as multiline. Width is known from the (fixed) modal width, so
    // we can size the modal height to fit the wrapped rows.
    let modal_width = 70.min(area.width);
    let inner_width = usize::from(modal_width).saturating_sub(2);
    let text_width = inner_width.saturating_sub(BRAIN_INPUT_MARGIN).max(1);
    let rows = wrap_input(&state.buffer, text_width);
    // Fixed chrome around the input: 2 border + 1 label/spacer + 1 footer.
    let max_input_rows = usize::from(area.height).saturating_sub(4).max(1);
    let input_rows = rows.len().clamp(1, max_input_rows.min(12));
    let modal_height = u16::try_from(input_rows + 4)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let modal = centered_rect(modal_width, modal_height, area);
    f.render_widget(Clear, modal);

    let title = state.about_task.as_ref().map_or_else(
        || "Message brain".to_owned(),
        |id| format!("Message brain about {id}"),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                title,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let input_h = u16::try_from(input_rows).unwrap_or(1);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),       // task label (dim) OR spacer
            Constraint::Length(input_h), // input (wrapped, multiline)
            Constraint::Min(0),          // spacer
            Constraint::Length(1),       // footer
        ])
        .split(inner);

    // Top row: dim task label when present, otherwise leave the spacer
    // empty so the input still has breathing room below the title.
    if let Some(label) = &state.task_label {
        let max_chars = usize::from(inner.width).saturating_sub(2).max(8);
        let label_text = crate::tasks::render::truncate(label, max_chars);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    label_text,
                    Style::default().fg(Color::Rgb(122, 134, 173)),
                ),
            ])),
            chunks[0],
        );
    }

    // Render the wrapped rows. When the buffer is taller than the input
    // area, show the tail so the cursor stays visible. The `> ` prompt
    // prefixes the first visible row; continuation rows are indented to
    // align under the text.
    let prompt_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::Rgb(192, 202, 245));
    let first_visible = rows.len().saturating_sub(input_rows);
    let input_lines: Vec<Line> = rows[first_visible..]
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let prefix = if first_visible + i == 0 { " > " } else { "   " };
            Line::from(vec![
                Span::styled(prefix, prompt_style),
                Span::styled(row.clone(), text_style),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(input_lines), chunks[1]);

    // The cursor is always at the end of the buffer, i.e. the end of the
    // last wrapped row, on the last visible input line.
    let last_len = rows.last().map_or(0, |r| r.chars().count());
    let cursor_x = chunks[1]
        .x
        .saturating_add(u16::try_from(BRAIN_INPUT_MARGIN).unwrap_or(0))
        .saturating_add(u16::try_from(last_len).unwrap_or(u16::MAX))
        .min(chunks[1].x + chunks[1].width.saturating_sub(1));
    let cursor_y = chunks[1]
        .y
        .saturating_add(u16::try_from(input_rows.saturating_sub(1)).unwrap_or(0));
    f.set_cursor_position((cursor_x, cursor_y));

    let key_style = Style::default()
        .fg(Color::Rgb(192, 202, 245))
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::Rgb(122, 134, 173));
    let footer = Line::from(vec![
        Span::raw(" "),
        Span::styled("Enter", key_style),
        Span::styled(" send  ", dim),
        Span::styled("⌥Enter", key_style),
        Span::styled(" newline  ", dim),
        Span::styled("^U", key_style),
        Span::styled(" clear  ", dim),
        Span::styled("Esc", key_style),
        Span::styled(" cancel", dim),
    ]);
    f.render_widget(Paragraph::new(footer), chunks[3]);
}
