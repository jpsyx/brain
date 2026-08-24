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
use crate::tui::model::BrainTab;

pub(crate) struct BrainPanelContext<'a> {
    pub(super) focused: bool,
    pub(super) tab_titles: Vec<String>,
    pub(super) active_tab: BrainTab,
    pub(super) active_is_skill_session: bool,
    pub(super) active_index: usize,
    pub(super) workspace_name: String,
    pub(super) session_title: Option<String>,
    pub(super) agent: String,
    pub(super) alert: Option<String>,
    pub(super) controller: Option<&'a mut AgentController>,
}

pub(crate) fn draw_brain(f: &mut Frame, context: &mut BrainPanelContext<'_>, area: Rect) {
    let focused = context.focused;
    let tab_titles = &context.tab_titles;
    let has_tabs = tab_titles.len() > 1;
    let active_tab = context.active_tab;
    let active_index = context.active_index;
    let alive = context
        .controller
        .as_ref()
        .is_some_and(|controller| controller.is_alive().unwrap_or(false));

    let border_color = if focused {
        Color::Rgb(125, 207, 255) // cyan accent — matches the rest of the palette
    } else {
        Color::Rgb(78, 92, 122) // very dim
    };
    let agent = context.agent.as_str();
    let title_status = panel_title(
        &context.workspace_name,
        context.session_title.as_deref(),
        agent,
        alive,
    );
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

    // With any ephemeral tab open, the top inner row is a tab strip; the
    // bottom row is always the help / status footer. The PTY fills what's left.
    let mut term_y = inner.y;
    let mut body_h = inner.height;
    if has_tabs && body_h > 0 {
        let tab_area = Rect {
            x: inner.x,
            y: term_y,
            width: inner.width,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(vec![tab_bar_line(tab_titles, active_index)]),
            tab_area,
        );
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
    if let Some(controller) = context.controller.as_mut() {
        if term_area.height > 0 && term_area.width > 0 {
            let _ = controller.resize(term_area.height, term_area.width);
        }
    }

    if let Some(screen) = context
        .controller
        .as_ref()
        .and_then(|controller| controller.terminal_screen().ok().flatten())
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
    let footer = match &context.alert {
        Some(alert) => Line::from(Span::styled(
            format!(" {alert}"),
            Style::default()
                .fg(Color::Rgb(255, 199, 119))
                .add_modifier(Modifier::BOLD),
        )),
        None if alive => footer_hint(
            active_tab,
            has_tabs,
            context.active_is_skill_session,
            key,
            dim,
        ),
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

/// The tab strip shown at the top of the brain panel while any ephemeral tab is
/// open: the main session, then one numbered tab per open tab, in the
/// order they were opened (matching their `Alt+<digit>` slots). The active tab is
/// bright; the others are dimmed.
fn tab_bar_line(titles: &[String], active_index: usize) -> Line<'static> {
    let active_style = Style::default()
        .fg(Color::Rgb(125, 207, 255))
        .add_modifier(Modifier::BOLD);
    let idle_style = Style::default().fg(Color::Rgb(122, 134, 173));
    let mut spans = vec![Span::raw(" ")];
    for (index, title) in titles.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!(" {} {title} ", index + 1),
            if index == active_index {
                active_style
            } else {
                idle_style
            },
        ));
    }
    Line::from(spans)
}

/// The normal (agent-alive) footer hint. Names the reliable way back to tasks
/// and, when an ephemeral tab is open, the tab-switch key. Skill sessions keep
/// their tab-specific close action (`^X` from the selected skill tab).
fn footer_hint(
    active: BrainTab,
    has_tabs: bool,
    active_is_skill_session: bool,
    key: Style,
    dim: Style,
) -> Line<'static> {
    let on_session = matches!(active, BrainTab::Session(_));
    let mut spans = vec![
        Span::raw(" "),
        Span::styled("Alt+H", key),
        Span::styled(" tasks", dim),
    ];
    if has_tabs {
        spans.push(Span::styled("   ", dim));
        spans.push(Span::styled("Alt+[ ]", key));
        spans.push(Span::styled(
            if on_session { " brain" } else { " sessions" },
            dim,
        ));
    }
    if !on_session || active_is_skill_session {
        spans.push(Span::styled("   ", dim));
        spans.push(Span::styled("^X", key));
        spans.push(Span::styled(
            if active_is_skill_session {
                " close tab"
            } else {
                " close brain"
            },
            dim,
        ));
    }
    Line::from(spans)
}

/// The panel's title: which brain you are in, which sub-view, which frontend.
///
/// The workspace name leads because a machine can have several workspaces and a
/// literal "Brain" named none of them — with `family` and `brain` both open, the
/// title was the one place that could tell them apart and didn't.
pub(crate) fn panel_title(
    workspace: &str,
    session_title: Option<&str>,
    agent: &str,
    alive: bool,
) -> String {
    let base = session_title.map_or_else(
        || format!("{workspace} · {agent}"),
        |title| format!("{workspace} · {title} · {agent}"),
    );
    if alive {
        base
    } else {
        format!("{base} exited")
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Style;

    use super::{footer_hint, panel_title};
    use crate::tui::model::{BrainTab, SessionTabId};

    #[test]
    fn the_main_tab_names_the_workspace_instead_of_the_product() {
        let title = panel_title("family", None, "Claude", true);

        assert_eq!(title, "family · Claude");
        assert!(!title.contains("Brain"));
    }

    #[test]
    fn a_skill_session_tab_names_its_session_and_still_names_the_workspace() {
        assert_eq!(
            panel_title("brain", Some("Daily triage"), "Codex", true),
            "brain · Daily triage · Codex"
        );
    }

    #[test]
    fn an_exited_frontend_is_still_reported_after_the_workspace() {
        assert_eq!(
            panel_title("family", None, "OpenCode", false),
            "family · OpenCode exited"
        );
        assert_eq!(
            panel_title("family", Some("Daily triage"), "Claude", false),
            "family · Daily triage · Claude exited"
        );
    }

    #[test]
    fn receiver_tabs_do_not_advertise_the_skill_session_close_shortcut() {
        let footer = footer_hint(
            BrainTab::Session(SessionTabId(7)),
            true,
            false,
            Style::default(),
            Style::default(),
        );

        assert!(footer.to_string().contains("Alt+[ ]"));
        assert!(!footer.to_string().contains("^X"));
        assert!(!footer.to_string().contains("close tab"));
    }
}
