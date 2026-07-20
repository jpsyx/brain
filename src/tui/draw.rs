//! The main draw entry plus tasks / brain panels and small draw helpers.

use super::*;

use crate::main_view::MainView;
use crate::state::PanelSide;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};
use tui_term::widget::PseudoTerminal;
use crate::pty_pane::PtyPane;
use crate::tasks::render::{
    compact_footer_line, search_bar_line, search_footer_line,
};

pub(crate) fn draw(f: &mut Frame, app: &mut App<'_>) {
    let area = f.area();

    // Top-level split: if the brain panel is open, it takes half the width on
    // its configured side; the active main view fills the rest. Closed → the
    // main view owns the full width.
    let (main_area, brain_area) = if app.brain.is_some() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        match app.panel_side {
            PanelSide::Right => (cols[0], Some(cols[1])),
            PanelSide::Left => (cols[1], Some(cols[0])),
        }
    } else {
        (area, None)
    };

    // Record the brain panel's rect so the mouse handler can hit-test the
    // wheel against it (None when the main view owns the full width).
    app.brain_rect = brain_area;

    match app.main_view {
        MainView::Tasks => draw_tasks(f, app, main_area),
        MainView::BrainSearch => crate::picker::draw_into(f, &mut app.search, main_area),
    }
    if let Some(brain_rect) = brain_area {
        draw_brain(f, app, brain_rect);
    }

    // Modals paint over the panels underneath. Help is app-level (either main
    // view); the tasks modals are only ever open in the tasks view; the
    // brain-search view's own palette / confirm overlays trail the chain.
    if let Some(help) = app.help.as_ref() {
        draw_help(f, help, area);
    } else if let Some(state) = app.palette.as_ref() {
        draw_palette(f, state, area);
    } else if let Some(brain_state) = app.brain_input.as_ref() {
        draw_brain_input(f, brain_state, area);
    } else if let Some(confirm) = app.confirm.as_ref() {
        draw_confirm(f, confirm, area);
    } else if let Some(picker) = app.link_picker.as_ref() {
        draw_link_picker(f, picker, area);
    } else if let Some(menu) = app.search.palette.as_ref() {
        crate::menu::draw_modal(f, menu, area);
    } else if let Some(c) = app.search.confirm.as_ref() {
        crate::confirm::draw_modal(f, c, area);
    }
}

pub(crate) fn draw_tasks(f: &mut Frame, app: &mut App<'_>, area: Rect) {
    let header_h = u16::try_from(app.header.len().min(3))
        .unwrap_or(u16::MAX)
        .max(1);
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
    let mut header = app.header.clone();
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

    // Footer slot: flash messages from the last palette action take
    // priority (cleared on next keystroke); otherwise show the compact
    // shortcut bar (subset + `?`).
    let footer = if let Some(flash) = &app.flash {
        flash_line(flash)
    } else if app.in_search {
        search_footer_line()
    } else {
        compact_footer_line(chunks[3].width, app.pending_count)
    };
    f.render_widget(Paragraph::new(vec![footer]), chunks[3]);
}

pub(crate) fn draw_brain(f: &mut Frame, app: &mut App<'_>, area: Rect) {
    let focused = app.focus == Panel::Brain;
    let alive = app.brain.as_ref().is_some_and(PtyPane::is_alive);

    let border_color = if focused {
        Color::Rgb(125, 207, 255) // cyan accent — matches the rest of the palette
    } else {
        Color::Rgb(78, 92, 122) // very dim
    };
    let title_status = if alive { "Brain" } else { "Brain · exited" };
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            title_status,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);
    let block = Block::default()
        .borders(Borders::LEFT | Borders::TOP)
        .border_style(Style::default().fg(border_color))
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Reserve the bottom row for the help / status footer.
    let term_h = inner.height.saturating_sub(1);
    let term_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: term_h,
    };
    let footer_area = Rect {
        x: inner.x,
        y: inner.y + term_h,
        width: inner.width,
        height: 1,
    };

    // Resize the PTY + parser to match the inner terminal area. No-op when
    // dimensions match, so this is safe to call every frame.
    if let Some(pty) = app.brain.as_mut() {
        if term_area.height > 0 && term_area.width > 0 {
            pty.resize(term_area.height, term_area.width);
        }
    }

    if let Some(pty) = app.brain.as_ref() {
        if let Ok(parser) = pty.parser.read() {
            let screen = parser.screen();
            let widget = PseudoTerminal::new(screen);
            f.render_widget(widget, term_area);

            // Place the real terminal cursor over the inner cursor when the
            // brain panel is focused; otherwise leave it on the tasks side.
            if focused && alive && !screen.hide_cursor() {
                let (row, col) = screen.cursor_position();
                let cx = term_area.x.saturating_add(col);
                let cy = term_area.y.saturating_add(row);
                f.set_cursor_position((cx, cy));
            }
        }
    }

    // Footer: a startup alert (resume failed → fresh chat) takes the row in
    // amber until the user switches focus; otherwise the normal hint shows.
    let key = Style::default()
        .fg(Color::Rgb(192, 202, 245))
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::Rgb(122, 134, 173));
    let footer = match &app.alert {
        Some(alert) => Line::from(Span::styled(
            format!(" {alert}"),
            Style::default()
                .fg(Color::Rgb(255, 199, 119))
                .add_modifier(Modifier::BOLD),
        )),
        None if alive => Line::from(vec![
            Span::raw(" "),
            Span::styled("Alt+H", key),
            Span::styled(" tasks", dim),
            Span::styled("   ", dim),
            Span::styled("^X", key),
            Span::styled(" close brain", dim),
        ]),
        // The event loop closes the panel as soon as claude exits, so this
        // shows for at most one frame before tasks goes full-width.
        None => Line::from(Span::styled(
            " claude exited — closing panel…",
            Style::default()
                .fg(Color::Rgb(255, 199, 119))
                .add_modifier(Modifier::BOLD),
        )),
    };
    f.render_widget(Paragraph::new(vec![footer]), footer_area);
}
/// Build the logical→visual row offset table. `heights[i]` is the number of
/// wrapped rows logical line `i` occupies once the Paragraph wraps it at the
/// content width. Returns a prefix sum of length `heights.len() + 1`: entry
/// `i` is the first visual row of logical line `i`, and the final entry is
/// the total visual row count. This is the bridge between the logical-line
/// bookkeeping (`task_line_ranges`, built width-agnostically) and the
/// wrapped rows the Paragraph actually paints.
pub(crate) fn visual_row_offsets(heights: &[u16]) -> Vec<u16> {
    let mut acc: u16 = 0;
    let mut out = Vec::with_capacity(heights.len() + 1);
    out.push(0);
    for &h in heights {
        acc = acc.saturating_add(h);
        out.push(acc);
    }
    out
}

/// Map a logical line range to its visual (wrapped) row range via `offsets`
/// (as built by [`visual_row_offsets`]). Indices are clamped into the table
/// so a stale range (computed before the latest rebuild) can't panic.
pub(crate) fn visual_range(
    offsets: &[u16],
    logical: std::ops::Range<usize>,
) -> std::ops::Range<u16> {
    if offsets.is_empty() {
        return 0..0;
    }
    let last = offsets.len() - 1;
    let start = offsets[logical.start.min(last)];
    let end = offsets[logical.end.min(last)];
    start..end
}

/// Compute the on-screen rectangle covering `app.selected_task`'s content
/// rows, clipped to the visible portion. Returns `None` when nothing is
/// selected or the selection is entirely off-screen (scrolled away).
/// Works in visual (wrapped) rows so a task sitting after a wrapped note
/// still highlights the right band.
pub(crate) fn selection_band_rect(app: &App<'_>, content_area: Rect) -> Option<Rect> {
    let sel = app.selected_task?;
    let range = app.task_line_ranges.get(sel)?;
    let vis = visual_range(&app.visual_row_offsets, range.clone());
    let start = vis.start;
    let end = vis.end;
    let scroll = app.scroll;
    let bottom = scroll.saturating_add(content_area.height);
    if end <= scroll || start >= bottom {
        return None;
    }
    let vis_start = start.max(scroll);
    let vis_end = end.min(bottom);
    let height = vis_end.saturating_sub(vis_start);
    if height == 0 {
        return None;
    }
    Some(Rect {
        x: content_area.x,
        y: content_area.y + (vis_start - scroll),
        width: content_area.width,
        height,
    })
}

/// Blend an RGB color 80% toward its current value, 20% toward white.
/// Non-RGB colors (named, indexed, reset) pass through unchanged — we
/// don't want to override a deliberately styled crossed-out / dimmed
/// cell with something it can't represent.
pub(crate) fn brighten_color(c: Color) -> Color {
    // Each channel: (orig * 8 + 255 * 2 + 5) / 10 → 80/20 blend with
    // white, rounded. Max value of the dividend is 255*8 + 510 + 5 =
    // 2555, divided by 10 = 255 — fits in u8 cleanly.
    #[allow(clippy::cast_possible_truncation)]
    fn blend(x: u8) -> u8 {
        ((u16::from(x) * 8 + 255 * 2 + 5) / 10) as u8
    }
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(blend(r), blend(g), blend(b)),
        other => other,
    }
}

/// Walk every cell in `band` and brighten its foreground color. Called
/// after the body Paragraph has written its span colors so we operate
/// on the actual final fg per cell.
pub(crate) fn brighten_band_text(f: &mut Frame, band: Rect) {
    let buf = f.buffer_mut();
    for y in band.y..band.y.saturating_add(band.height) {
        for x in band.x..band.x.saturating_add(band.width) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                let brighter = brighten_color(cell.fg);
                cell.set_fg(brighter);
            }
        }
    }
}

pub(crate) fn flash_line(flash: &FlashKind) -> Line<'static> {
    match flash {
        FlashKind::Info(msg) => Line::from(vec![
            Span::raw(" "),
            Span::styled(
                msg.clone(),
                Style::default()
                    .fg(Color::Rgb(158, 206, 106)) // green
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        FlashKind::Error(msg) => Line::from(vec![
            Span::raw(" "),
            Span::styled(
                msg.clone(),
                Style::default()
                    .fg(Color::Rgb(247, 118, 142)) // pink-red
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    }
}

/// Center a sub-Rect of the given width/height inside `area`, clamped so
/// the modal always fits even on tiny terminals.
pub(crate) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect { x, y, width: w, height: h }
}
