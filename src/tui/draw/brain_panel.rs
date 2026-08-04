//! Rendering the brain panel: the bordered agent PTY (via `tui-term`), its
//! focus-aware title/border, cursor placement, and the footer that shows the
//! resume alert or the normal hint.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use tui_term::widget::PseudoTerminal;

use crate::agent::AgentController;
use crate::tui::*;

pub(crate) fn draw_brain(f: &mut Frame, app: &mut App<'_>, area: Rect) {
    let focused = app.focus == Panel::Brain;
    let has_triage = app.triage_brain.is_some();
    let active_tab = app.effective_brain_tab();
    let alive = app
        .active_brain_controller()
        .is_some_and(AgentController::is_alive);

    let border_color = if focused {
        Color::Rgb(125, 207, 255) // cyan accent — matches the rest of the palette
    } else {
        Color::Rgb(78, 92, 122) // very dim
    };
    let agent = app.agent_kind.label();
    let base_title = match active_tab {
        BrainTab::Main => format!("Brain · {agent}"),
        BrainTab::Triage => format!("Daily triage · {agent}"),
    };
    let title_status = if alive {
        base_title
    } else {
        format!("{base_title} exited")
    };
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

    // With a triage tab open, the top inner row is a tab strip; the bottom row
    // is always the help / status footer. The PTY fills what's left.
    let mut term_y = inner.y;
    let mut body_h = inner.height;
    if has_triage && body_h > 0 {
        let tab_area = Rect {
            x: inner.x,
            y: term_y,
            width: inner.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(vec![tab_bar_line(active_tab)]), tab_area);
        term_y = term_y.saturating_add(1);
        body_h = body_h.saturating_sub(1);
    }
    let footer_h = body_h.min(1);
    let term_h = body_h.saturating_sub(footer_h);
    let term_area = Rect {
        x: inner.x,
        y: term_y,
        width: inner.width,
        height: term_h,
    };
    let footer_area = Rect {
        x: inner.x,
        y: term_y + term_h,
        width: inner.width,
        height: footer_h,
    };

    // Resize the active PTY + parser to match the inner terminal area. No-op
    // when dimensions match, so this is safe to call every frame.
    if let Some(controller) = app.active_brain_controller_mut() {
        if term_area.height > 0 && term_area.width > 0 {
            controller.resize(term_area.height, term_area.width);
        }
    }

    if let Some(screen) = app
        .active_brain_controller()
        .and_then(AgentController::terminal_screen)
    {
        if let Ok(parser) = screen.read() {
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
        None if alive => footer_hint(active_tab, has_triage, key, dim),
        // The event loop closes the panel as soon as the agent exits, so this
        // shows for at most one frame before tasks goes full-width.
        None => Line::from(Span::styled(
            format!(" {agent} exited: closing panel..."),
            Style::default()
                .fg(Color::Rgb(255, 199, 119))
                .add_modifier(Modifier::BOLD),
        )),
    };
    f.render_widget(Paragraph::new(vec![footer]), footer_area);
}

/// The two-tab strip shown at the top of the brain panel while a daily-triage
/// session is running. The active tab is bright; the other is dimmed.
fn tab_bar_line(active: BrainTab) -> Line<'static> {
    let active_style = Style::default()
        .fg(Color::Rgb(125, 207, 255))
        .add_modifier(Modifier::BOLD);
    let idle_style = Style::default().fg(Color::Rgb(122, 134, 173));
    let style_for = |tab: BrainTab| {
        if tab == active {
            active_style
        } else {
            idle_style
        }
    };
    Line::from(vec![
        Span::raw(" "),
        Span::styled(" 1 Brain ", style_for(BrainTab::Main)),
        Span::raw(" "),
        Span::styled(" 2 Daily triage ", style_for(BrainTab::Triage)),
    ])
}

/// The normal (agent-alive) footer hint. Names the reliable way back to tasks
/// and, when a triage tab is open, the tab-switch key and the tab-specific
/// close action (`^X` closes only the triage tab from the triage tab).
fn footer_hint(active: BrainTab, has_triage: bool, key: Style, dim: Style) -> Line<'static> {
    let mut spans = vec![
        Span::raw(" "),
        Span::styled("Alt+H", key),
        Span::styled(" tasks", dim),
    ];
    if has_triage {
        let switch_label = match active {
            BrainTab::Main => " triage",
            BrainTab::Triage => " brain",
        };
        spans.push(Span::styled("   ", dim));
        spans.push(Span::styled("Alt+[ ]", key));
        spans.push(Span::styled(switch_label, dim));
    }
    spans.push(Span::styled("   ", dim));
    spans.push(Span::styled("^X", key));
    spans.push(Span::styled(
        if has_triage && active == BrainTab::Triage {
            " close tab"
        } else {
            " close brain"
        },
        dim,
    ));
    Line::from(spans)
}
