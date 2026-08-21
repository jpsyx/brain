//! Rendering the search panel: header / input / separator / list / footer,
//! Used inside a bordered sub-rect by the persistent shell.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Paragraph,
};

use crate::render;

use super::{App, DisplayRow};

/// Render the search panel into `area`.
///
/// Draws header / input / separator / list / footer. Shell overlays are drawn
/// by `tui::draw` after both panels.
pub fn draw_into(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Length(1), // input
            Constraint::Length(1), // separator
            Constraint::Min(1),    // list
            Constraint::Length(1), // footer
        ])
        .split(area);

    let scope = scope_label(app);
    f.render_widget(
        Paragraph::new(render::header_line(
            &scope,
            app.entries.len(),
            app.matches.len(),
        )),
        chunks[0],
    );
    f.render_widget(Paragraph::new(render::input_line(&app.query)), chunks[1]);
    f.render_widget(
        Paragraph::new(render::separator_line(area.width as usize)),
        chunks[2],
    );

    draw_list(f, app, chunks[3]);

    f.render_widget(Paragraph::new(render::footer_line()), chunks[4]);
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    if app.matches.is_empty() {
        f.render_widget(
            Paragraph::new(render::empty_line(app.query.is_empty())),
            area,
        );
        return;
    }

    let height = area.height as usize;
    app.ensure_visible(height);

    let lines = app
        .display_rows
        .iter()
        .skip(app.top)
        .take(height)
        .map(|row| match row {
            DisplayRow::Header(bucket, count) => {
                render::section_header_line(bucket.label(), *count)
            }
            DisplayRow::Match(i) => {
                let m = &app.matches[*i];
                render::entry_line(
                    &app.entries[m.entry_idx].display,
                    &m.highlight_bytes,
                    *i == app.selected,
                )
            }
        })
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines), area);
}

fn scope_label(app: &App) -> String {
    if app.query.is_empty() {
        "search".to_owned()
    } else {
        format!("search · '{}'", app.query)
    }
}
