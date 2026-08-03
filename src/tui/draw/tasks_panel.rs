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

pub(crate) fn draw_tasks(f: &mut Frame, app: &mut App<'_>, area: Rect) {
    let mut header = app.header.clone();
    if let Some(user_id) = app.assignment_filter.as_ref() {
        let name = app
            .assignment
            .users()
            .iter()
            .find(|user| &user.id == user_id)
            .map_or_else(|| user_id.as_str(), |user| user.name.as_str());
        header.push(assignee_filter_line(name, user_id.as_str()));
    }
    let header_h = tasks_header_height(header.len());
    let search_h: u16 = u16::from(app.show_search_bar());

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
    if app.brain.is_some() && app.focus != Panel::Tasks {
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
    let split_pane_open = app.brain.is_some();
    let tasks_focused = app.focus == Panel::Tasks;
    let border_color = if split_pane_open && tasks_focused {
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
    app.last_inner_height = content_area.height;
    // Measure how each logical line wraps at the current content width and
    // build the logical→visual row table. The body Paragraph wraps each
    // line independently (newlines reset wrapping), so per-line `line_count`
    // matches the rows it ultimately paints. Recomputed every frame because
    // the width changes whenever the brain panel splits the screen.
    let wrap_width = content_area.width.max(1);
    let heights: Vec<u16> = app
        .body_lines
        .iter()
        .map(|line| {
            let measured = Paragraph::new(line.clone())
                .wrap(Wrap { trim: false })
                .line_count(wrap_width);
            u16::try_from(measured).unwrap_or(u16::MAX).max(1)
        })
        .collect();
    app.visual_row_offsets = visual_row_offsets(&heights);
    app.last_content_rows = app.visual_row_offsets.last().copied().unwrap_or(0);
    // Now that we know the visible height, scroll so the selected task is
    // fully on-screen. Cheap to call every frame; idempotent.
    app.ensure_selected_visible();
    app.clamp();

    // Paint the selection-highlight band BEFORE rendering the body so the
    // body's spans (no `bg` set) layer on top of it. `Cell::set_style`
    // patches rather than overwrites, so the bg survives where the
    // spans don't override it.
    let band = selection_band_rect(app, content_area);
    if let Some(band) = band {
        f.render_widget(
            Block::default().style(Style::default().bg(SELECTED_BG)),
            band,
        );
    }

    let body = Paragraph::new(app.body_lines.clone())
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(body, content_area);

    // Second pass: brighten the fg of every cell inside the selection band
    // so the text on the highlighted row reads warmer than its neighbors.
    // Done AFTER body render so we mutate the final span colors directly.
    if let Some(band) = band {
        brighten_band_text(f, band);
    }

    // Scrollbar
    let max_offset = usize::from(app.max_scroll());
    let mut scroll_state = ScrollbarState::new(max_offset).position(usize::from(app.scroll));
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
        let bar = search_bar_line(&app.query, app.visible_tasks.len(), app.base_tasks.len());
        f.render_widget(Paragraph::new(bar), bar_area);

        // Only show the tasks-panel cursor when the tasks panel has focus;
        // when the brain panel is focused, the cursor belongs over there.
        if app.in_search && app.focus == Panel::Tasks {
            // Cursor goes after " / " (3 cols) + the query glyphs.
            let prefix: u16 = 3;
            let q_len = u16::try_from(app.query.chars().count()).unwrap_or(u16::MAX);
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
    let footer = if let Some(status) = status_override_line(
        app.flash.as_ref(),
        app.sync_status.as_deref(),
        app.persistent_warning.as_deref(),
    ) {
        status
    } else if app.in_search {
        search_footer_line()
    } else {
        compact_footer_line(chunks[3].width, app.pending_count)
    };
    f.render_widget(Paragraph::new(vec![footer]), chunks[3]);
}

pub(crate) fn tasks_header_height(line_count: usize) -> u16 {
    u16::try_from(line_count).unwrap_or(u16::MAX).max(1)
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
