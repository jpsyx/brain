//! Rendering the tasks main-view panel: header, bordered body with the
//! wrapped task list, selection-highlight band + fg-brighten pass, scrollbar,
//! the `/` search bar, and the footer (flash / search hint / compact bar).

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::tasks::render::{compact_footer_line, search_bar_line, search_footer_line};
use crate::tui::*;

pub(crate) struct TasksPanelContext<'a> {
    pub(crate) split_pane_open: bool,
    pub(crate) focused: bool,
    pub(crate) flash: Option<&'a FlashKind>,
    pub(crate) sync_status: Option<&'a str>,
    pub(crate) persistent_warning: Option<&'a str>,
}

pub(crate) fn draw_tasks(
    f: &mut Frame,
    tasks: &mut TasksState,
    context: &TasksPanelContext<'_>,
    area: Rect,
) {
    let render = tasks.render_state();
    let mut header = tasks_header_lines(
        render.header,
        render.assignment_users,
        render.assignment_filter,
    );
    let header_h = tasks_header_height(header.len());
    let search_h: u16 = u16::from(render.show_search_bar);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Min(1),
            Constraint::Length(search_h),
            Constraint::Length(1),
        ])
        .split(area);

    // Header. When the brain panel is open and has focus, fade the whole
    // title line (TASKS · view title · count) so the dim title + dim top
    // border together signal "tasks is not the active panel" — Alt+H
    // restores both to color. Mirrors the brain panel title, which grays
    // when it loses focus.
    if context.split_pane_open && !context.focused {
        if let Some(line) = header.first_mut() {
            for span in &mut line.spans {
                span.style = span.style.fg(Color::Rgb(78, 92, 122));
            }
        }
    }
    f.render_widget(Paragraph::new(header), chunks[0]);

    // Body block. When the right panel is visible AND tasks has focus,
    // we color the top hairline purple so the user can tell where
    // input is going — mirrors the cyan / green top border on the
    // brain panel. With no split there's no ambiguity, so we keep the
    // unobtrusive dim hairline.
    let border_color = if context.split_pane_open && context.focused {
        // A brightened violet so the focused tasks border pops as much as
        // the brain panel's cyan. The muted brand purple (187,154,247) read
        // too dim here, so we lift it rather than reuse ACCENT_PURPLE.
        Color::Rgb(208, 175, 255)
    } else {
        Color::Rgb(78, 92, 122)
    };
    let body_area = chunks[1];
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(body_area);
    f.render_widget(block, body_area);

    // Reserve 1 column for the scrollbar.
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width.saturating_sub(1),
        height: inner.height,
    };
    // Measure how each logical line wraps at the current content width and
    // build the logical→visual row table. The body Paragraph wraps each
    // line independently (newlines reset wrapping), so per-line `line_count`
    // matches the rows it ultimately paints. Recomputed every frame because
    // the width changes whenever the brain panel splits the screen.
    let wrap_width = content_area.width.max(1);
    let heights: Vec<u16> = tasks
        .render_state()
        .body_lines
        .iter()
        .map(|line| {
            let measured = Paragraph::new(line.clone())
                .wrap(Wrap { trim: false })
                .line_count(wrap_width);
            u16::try_from(measured).unwrap_or(u16::MAX).max(1)
        })
        .collect();
    tasks.update_body_layout(content_area.height, &heights);

    // Paint the selection-highlight band BEFORE rendering the body so the
    // body's spans (no `bg` set) layer on top of it. `Cell::set_style`
    // patches rather than overwrites, so the bg survives where the
    // spans don't override it.
    let band = tasks.selection_band_rect(content_area);
    if let Some(band) = band {
        f.render_widget(
            Block::default().style(Style::default().bg(SELECTED_BG)),
            band,
        );
    }

    let render = tasks.render_state();
    let body = Paragraph::new(render.body_lines.to_vec())
        .wrap(Wrap { trim: false })
        .scroll((render.scroll, 0));
    f.render_widget(body, content_area);

    // Second pass: brighten the fg of every cell inside the selection band
    // so the text on the highlighted row reads warmer than its neighbors.
    // Done AFTER body render so we mutate the final span colors directly.
    if let Some(band) = band {
        brighten_band_text(f, band);
    }

    // Scrollbar
    let max_offset = usize::from(render.max_scroll);
    let mut scroll_state = ScrollbarState::new(max_offset).position(usize::from(render.scroll));
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .style(Style::default().fg(Color::DarkGray));
    let scrollbar_area = Rect {
        x: body_area.x,
        y: body_area.y + 1,
        width: body_area.width,
        height: body_area.height.saturating_sub(1),
    };
    f.render_stateful_widget(scrollbar, scrollbar_area, &mut scroll_state);

    // Search bar (only when search-mode active or filter is set).
    if search_h > 0 {
        let bar_area = chunks[2];
        let bar = search_bar_line(render.query, render.visible_count, render.base_count);
        f.render_widget(Paragraph::new(bar), bar_area);

        // Only show the tasks-panel cursor when the tasks panel has focus;
        // when the brain panel is focused, the cursor belongs over there.
        if render.in_search && context.focused {
            // Cursor goes after " / " (3 cols) + the query glyphs.
            let prefix: u16 = 3;
            let q_len = u16::try_from(render.query.chars().count()).unwrap_or(u16::MAX);
            let max_x = bar_area.x + bar_area.width.saturating_sub(1);
            let cx = bar_area
                .x
                .saturating_add(prefix)
                .saturating_add(q_len)
                .min(max_x);
            f.set_cursor_position((cx, bar_area.y));
        }
    }

    // Footer slot: a transient palette flash takes priority until the next
    // keystroke. A persistent receiver warning then returns; otherwise show
    // the usual search hint or compact shortcut bar.
    let footer = status_override_line(
        context.flash,
        context.sync_status,
        context.persistent_warning,
    )
    .unwrap_or_else(|| {
        if render.in_search {
            search_footer_line()
        } else {
            compact_footer_line(chunks[3].width, render.pending_count)
        }
    });
    f.render_widget(Paragraph::new(vec![footer]), chunks[3]);
}

pub(crate) fn tasks_header_height(line_count: usize) -> u16 {
    u16::try_from(line_count).unwrap_or(u16::MAX).max(1)
}

pub(crate) fn tasks_header_lines(
    static_header: &[Line<'static>],
    users: &[crate::tasks::task::AssignmentUser],
    assignment_filter: Option<&crate::users::UserId>,
) -> Vec<Line<'static>> {
    let mut header = static_header.to_vec();
    if let Some(user_id) = assignment_filter {
        let name = users
            .iter()
            .find(|user| &user.id == user_id)
            .map_or_else(|| user_id.as_str(), |user| user.name.as_str());
        header.push(assignee_filter_line(name, user_id.as_str()));
    }
    header
}

pub(crate) fn assignee_filter_line(name: &str, user_id: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "  ASSIGNEE  ",
            Style::default()
                .fg(Color::Rgb(125, 207, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{name} ({user_id})"),
            Style::default().fg(Color::Rgb(192, 202, 245)),
        ),
    ])
}
