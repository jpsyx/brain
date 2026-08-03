//! Screen chrome around the task body.
//!
//! Renders the header banner (title, view strip, active-filter chips), the
//! normal and search-mode footers, the live search bar, and the empty-state
//! body for a zero-match search.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::tasks::cli::Cli;
use crate::tasks::shortcuts;
use crate::tasks::view::{View, ViewSpec};

use super::style::{
    ACCENT_CYAN, ACCENT_PURPLE, DUE_TODAY, TEXT_DIM, TEXT_PRIMARY, TEXT_VERY_DIM, sep, very_dim,
};

#[must_use]
pub fn header_lines(view: &ViewSpec, cli: &Cli, active_view: Option<View>) -> Vec<Line<'static>> {
    let title_style = Style::new().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD);
    let primary_style = Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD);
    let dim_style = Style::new().fg(TEXT_DIM);

    let mut top: Vec<Span<'static>> = vec![
        Span::raw(" "),
        Span::styled("TASKS", title_style),
        sep(),
        Span::styled(view.title.clone(), primary_style),
    ];
    if !view.subtitle.is_empty() {
        top.push(sep());
        top.push(Span::styled(view.subtitle.clone(), dim_style));
    }
    top.push(sep());
    top.push(Span::styled(
        format!("{} of {}", view.tasks.len(), view.total),
        dim_style,
    ));

    let mut lines = vec![Line::from(top)];
    lines.push(view_strip_line(active_view));
    let chips = active_filter_chips(cli);
    if !chips.is_empty() {
        lines.push(Line::from(vec![
            Span::raw(" "),
            very_dim("filters · "),
            Span::styled(chips.join(" · "), Style::new().fg(DUE_TODAY)),
        ]));
    }
    lines
}

/// One-line strip showing all Tab-cycle views with the active one highlighted.
fn view_strip_line(active: Option<View>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(View::CYCLE.len() * 2 + 2);
    spans.push(Span::raw(" "));
    spans.push(very_dim("view · "));
    for (i, v) in View::CYCLE.iter().enumerate() {
        if i > 0 {
            spans.push(very_dim(" · "));
        }
        let is_active = active == Some(*v);
        let style = if is_active {
            Style::new().fg(DUE_TODAY).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(TEXT_DIM)
        };
        spans.push(Span::styled(v.label().to_owned(), style));
    }
    Line::from(spans)
}

fn active_filter_chips(cli: &Cli) -> Vec<String> {
    let f = &cli.filters;
    let mut chips: Vec<String> = Vec::new();
    if let Some(v) = f.hard_deadline {
        chips.push(format!("hard_deadline={v}"));
    }
    if let Some(v) = &f.status {
        chips.push(format!("status={v}"));
    }
    if let Some(v) = &f.priority {
        chips.push(format!("priority={v}"));
    }
    if let Some(v) = &f.task_type {
        chips.push(format!("type={v}"));
    }
    if let Some(v) = &f.project {
        chips.push(format!("project={v}"));
    }
    if let Some(v) = &f.energy {
        chips.push(format!("energy={v}"));
    }
    if let Some(v) = &f.context {
        chips.push(format!("context={v}"));
    }
    if let Some(v) = &f.assigned_to {
        chips.push(format!("assigned_to={v}"));
    }
    if f.past_due {
        chips.push("past-due".into());
    }
    if f.mit {
        chips.push("mit".into());
    }
    if f.stale {
        chips.push("stale".into());
    }
    if f.no_due {
        chips.push("no-due".into());
    }
    if f.blocked {
        chips.push("blocked".into());
    }
    if f.include_done {
        chips.push("+done".into());
    }
    if f.include_deferred {
        chips.push("+deferred".into());
    }
    if let Some(q) = &f.search {
        chips.push(format!("search '{q}'"));
    }
    chips
}

/// The compact footer.
///
/// The curated `shortcuts::footer_subset()` rendered
/// left-to-right until the row is nearly full, then a `…  ? all` tail that
/// points at the help modal (`?`). Width-aware so it never overflows the
/// panel — when the brain panel halves the width, fewer chips fit and the
/// ellipsis appears earlier; the full list is always one `?` away.
///
/// This renders straight off the `shortcuts` table, the single source of
/// truth, so adding a binding there updates the footer automatically.
#[must_use]
pub fn compact_footer_line(width: u16, pending_count: Option<usize>) -> Line<'static> {
    let key = Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD);
    let lbl = Style::new().fg(TEXT_DIM);
    let dot = Style::new().fg(TEXT_VERY_DIM);

    // The tail (`…   Alt+S all shortcuts`) is always shown; reserve room for it.
    let tail = "…   Alt+S all shortcuts";
    let tail_w = tail.chars().count() + 1; // + leading space
    let budget = usize::from(width);

    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
    let mut used = 1usize;

    // Vim-style count prefix indicator, shown while digits are pending.
    if let Some(n) = pending_count {
        let chip = format!("{n}× ");
        used += chip.chars().count();
        spans.push(Span::styled(
            chip,
            Style::new().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ));
    }

    for s in shortcuts::footer_subset() {
        // `keys label   ` — width of this chunk including the trailing gap.
        let chunk_w = s.keys.chars().count() + 1 + s.label.chars().count() + 3;
        if used + chunk_w + tail_w > budget {
            break;
        }
        spans.push(Span::styled(s.keys, key));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(s.label, lbl));
        spans.push(Span::styled("   ", dot));
        used += chunk_w;
    }

    spans.push(Span::styled("…   ", dot));
    spans.push(Span::styled("Alt+S", key));
    spans.push(Span::styled(" all shortcuts", lbl));
    Line::from(spans)
}

#[must_use]
pub fn search_footer_line() -> Line<'static> {
    let key = Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD);
    let lbl = Style::new().fg(TEXT_DIM);
    let dot = Style::new().fg(TEXT_VERY_DIM);
    Line::from(vec![
        Span::raw(" "),
        Span::styled("type", key),
        Span::styled(" to filter", lbl),
        Span::styled("   ", dot),
        Span::styled("⌫", key),
        Span::styled(" delete", lbl),
        Span::styled("   ", dot),
        Span::styled("Esc", key),
        Span::styled(" cancel", lbl),
        Span::styled("   ", dot),
        Span::styled("Enter", key),
        Span::styled(" keep filter", lbl),
        Span::styled("   ", dot),
        Span::styled("^⏎", key),
        Span::styled(" actions", lbl),
        Span::styled("   ", dot),
        Span::styled("^D", key),
        Span::styled(" done", lbl),
        Span::styled("   ", dot),
        Span::styled("^U", key),
        Span::styled(" clear", lbl),
        Span::styled("   ", dot),
        Span::styled("↑↓ PgUp/Dn", key),
        Span::styled(" scroll", lbl),
    ])
}

/// The live search input line shown above the footer.
///
/// Shown while search mode is active (or while a filter is set). The terminal
/// cursor is positioned over the trailing space externally via
/// `Frame::set_cursor_position`.
#[must_use]
pub fn search_bar_line(query: &str, matches: usize, total: usize) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(
            "/",
            Style::new().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            query.to_owned(),
            Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        very_dim(format!("{matches} of {total} matches")),
    ])
}

/// Body when an active search yields zero matches.
#[must_use]
pub fn no_matches_lines(query: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  No matches for '{query}'."),
            Style::new().fg(DUE_TODAY).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Edit with backspace, or press Esc to clear.",
            Style::new().fg(TEXT_DIM),
        )),
    ]
}
