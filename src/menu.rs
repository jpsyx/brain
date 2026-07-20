//! The top-level command palette: the list of every action `brain` can run.
//!
//! It is **not** a standalone screen. The picker / two-panel TUI opens it
//! with `Ctrl-p` and renders it as a modal overlay (`draw_modal`) on top of
//! the current search, driving it through this module's pure `MenuApp` +
//! `handle_key`. Esc closes the overlay; Enter confirms a `Choice` the host
//! acts on. The rows: per-bucket pickers, global search, go-to-root, the
//! claude-msg path, open tasks, and the layout-swap toggle.
//!
//! The palette doubles as a text input: typing filters the rows. Each row's
//! matchable text includes its 1-based number (`"1. Message brain"`), so the
//! user can type the digit *or* any word from the label and the list
//! narrows to the hits. `↑`/`↓`, `Ctrl-k`/`Ctrl-j`, and `Ctrl-p`/`Ctrl-n`
//! cycle the (filtered) list.
//!
//! One row carries a **dynamic** label: the layout toggle reads "Move brain
//! panel to the left" or "...right" depending on where the panel currently
//! sits, so the palette is built per-open with the current `PanelSide`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::render;
use crate::state::PanelSide;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Convert the highlighted markdown file to a colocated PDF. Only offered
    /// as a row when a `.md` file is selected (its label carries the
    /// filename), so it is a *conditional* choice, not part of `STATIC_ITEMS`.
    CreatePdf,
    /// Open the highlighted file (the same action as `Enter` in the picker).
    /// Offered as a row only when a *file* is highlighted (its label carries
    /// the filename), so it's a *conditional* choice, not part of
    /// `STATIC_ITEMS`. There's no file to open on a directory.
    OpenFile,
    /// Open the highlighted entry's directory (the same action as
    /// `Ctrl-Enter` / reveal-in-Finder). A file resolves to its parent
    /// directory, a directory to itself. Offered whenever an entry is
    /// highlighted (its label carries the bucket-relative directory path), so
    /// it's a *conditional* choice, not part of `STATIC_ITEMS`.
    OpenDir,
    /// Move the highlighted entry (file or directory) to the Trash. Offered as
    /// a row whenever something is selected (its label carries the filename),
    /// so it's a *conditional* choice, not part of `STATIC_ITEMS`.
    Delete,
    Msg,
    OpenTasks,
    SearchProjects,
    SearchAreas,
    SearchResources,
    SearchArchive,
    GlobalSearch,
    /// Swap which side the brain panel sits on. Only meaningful in the
    /// persistent two-panel TUI; a no-op for the one-shot picker.
    ToggleLayout,
}

/// The static rows, in display order. The layout-toggle row is appended
/// separately because its label depends on the current panel side.
const STATIC_ITEMS: &[(Choice, &str)] = &[
    (Choice::Msg, "Message brain"),
    (Choice::OpenTasks, "Open tasks"),
    (Choice::SearchProjects, "Search projects"),
    (Choice::SearchAreas, "Search areas"),
    (Choice::SearchResources, "Search resources"),
    (Choice::SearchArchive, "Search archive"),
    (Choice::GlobalSearch, "Global search"),
];

/// The label for the layout-toggle row: it names the direction the panel
/// would move, i.e. the *opposite* of where it sits now.
#[must_use]
pub const fn layout_choice_label(side: PanelSide) -> &'static str {
    match side {
        PanelSide::Right => "Move brain panel to the left",
        PanelSide::Left => "Move brain panel to the right",
    }
}

/// The most characters of a *filename* we show in a contextual palette row
/// (`Create PDF for '…'`, `Open file '…'`, `Delete '…'`) before eliding. Caps
/// how far a single name can stretch the (content-sized, see [`palette_width`])
/// modal. Shared by those rows so they elide identically.
const LABEL_MAX_FILENAME: usize = 24;

/// The most characters of a *directory path* we show in the `Open dir '…'` row
/// before middle-eliding. A touch wider than a filename because a path packs in
/// more meaning per char (category + trailing segments); the shorter `Open dir`
/// prefix keeps the row from growing the modal despite the extra budget.
const LABEL_MAX_DIR: usize = 26;

/// Shorten a filename to fit a palette row: a head, an ellipsis, and a tail
/// that is always the **full extension** (e.g. `…mp4`, never `…p4`), so the
/// file type stays legible. Names without a usable extension keep the last two
/// chars, e.g. `really-long-note-name-here.md` → `really-long-note-h...md`.
fn truncate_label_filename(name: &str, max: usize) -> String {
    const ELLIPSIS: &str = "...";
    const DEFAULT_TAIL: usize = 2;
    let count = name.chars().count();
    if count <= max {
        return name.to_owned();
    }
    // Keep the whole extension as the tail, as long as a non-empty head still
    // fits after it; otherwise fall back to the last two chars.
    let tail_len = file_extension(name)
        .map(|ext| ext.chars().count())
        .filter(|&ext| ext + ELLIPSIS.len() < max)
        .unwrap_or(DEFAULT_TAIL)
        .max(DEFAULT_TAIL);
    let head_len = max.saturating_sub(ELLIPSIS.len() + tail_len);
    let head: String = name.chars().take(head_len).collect();
    let tail: String = name.chars().skip(count - tail_len).collect();
    format!("{head}{ELLIPSIS}{tail}")
}

/// The file extension (the chars after the last `.`), or `None` for a name with
/// no dot, a leading-dot dotfile (`.bashrc`), or a trailing dot (`name.`).
fn file_extension(name: &str) -> Option<&str> {
    name.rfind('.')
        .filter(|&dot| dot > 0)
        .map(|dot| &name[dot + 1..])
        .filter(|ext| !ext.is_empty())
}

/// Shorten a bucket-relative directory path to fit a palette row. Unlike a
/// filename (elided head + tail), a path keeps its leading **category**
/// segment (`projects`/`areas`/`resources`/`archive`) and drops the *middle*,
/// so the tail — the parts nearest the entry — stays readable, e.g.
/// `resources/a/b/c/final/parts` → `resources/.../final/parts`.
///
/// When over `max`, the head is `<category>/...` and the tail is as many of
/// the path's trailing chars as fit (pure char-count, so a cut can land
/// mid-segment). A single-segment path that overflows falls back to the
/// filename-style head+tail elision.
fn truncate_label_dir(rel: &str, max: usize) -> String {
    const MID: &str = "/...";
    if rel.chars().count() <= max {
        return rel.to_owned();
    }
    let Some(slash) = rel.find('/') else {
        return truncate_label_filename(rel, max);
    };
    let category = &rel[..slash];
    // `rest` leads with '/', so `<category>` + `/...` + `<rest tail>` reads as
    // `category/.../tail` when the tail happens to start at a separator.
    let rest = &rel[slash..];
    let prefix = category.chars().count() + MID.chars().count();
    let budget = max.saturating_sub(prefix);
    let rest_count = rest.chars().count();
    let tail: String = rest.chars().skip(rest_count.saturating_sub(budget)).collect();
    format!("{category}{MID}{tail}")
}

/// The "Create PDF" row label for a given markdown filename, with the
/// filename elided if it would overflow the palette row.
#[must_use]
pub fn create_pdf_label(filename: &str) -> String {
    format!(
        "Create PDF for '{}'",
        truncate_label_filename(filename, LABEL_MAX_FILENAME)
    )
}

/// The "Open file" row label for a given filename, elided with the same
/// threshold and head+tail logic as the "Create PDF" row.
#[must_use]
pub fn open_file_label(filename: &str) -> String {
    format!(
        "Open file '{}'",
        truncate_label_filename(filename, LABEL_MAX_FILENAME)
    )
}

/// The "Open dir" row label for a bucket-relative directory path, elided with
/// the middle-ellipsis path logic.
#[must_use]
pub fn open_dir_label(rel_dir: &str) -> String {
    format!("Open dir '{}'", truncate_label_dir(rel_dir, LABEL_MAX_DIR))
}

/// The "Delete" row label for a given filename, elided with the same
/// threshold as the "Create PDF" row so the two contextual rows line up.
#[must_use]
pub fn delete_label(filename: &str) -> String {
    format!(
        "Delete '{}'",
        truncate_label_filename(filename, LABEL_MAX_FILENAME)
    )
}

/// The contextual targets for the highlighted entry.
///
/// These drive the conditional palette rows: each field is the pre-formatted
/// text for that row's label (a filename, or a bucket-relative directory
/// path), or `None` when the row shouldn't appear for the current selection.
/// Named fields (rather than a row of same-typed `Option`s) keep the call
/// site unambiguous.
#[derive(Debug, Default, Clone)]
pub struct Targets {
    /// Highlighted markdown filename → "Create PDF for '…'".
    pub pdf: Option<String>,
    /// Highlighted file's filename → "Open file '…'" (files only).
    pub open_file: Option<String>,
    /// Highlighted entry's bucket-relative directory → "Open directory '…'".
    pub open_dir: Option<String>,
    /// Highlighted entry's filename (any kind) → "Delete '…'".
    pub delete: Option<String>,
}

/// The full ordered row list for a given panel side: the static rows plus
/// the dynamically-labeled layout toggle at the end. `include_msg` controls
/// whether the "Message brain" row is offered — the persistent shell hides
/// it while the brain panel is already open (there's nothing to open), and
/// shows it (to re-open the panel) once it's closed. The one-shot picker
/// always includes it.
fn items(side: PanelSide, include_msg: bool, targets: &Targets) -> Vec<(Choice, String)> {
    let mut v: Vec<(Choice, String)> = Vec::new();
    // The contextual entry-action rows lead the list so a common action is
    // the default-selected one on open. "Create PDF" keeps the lead when a
    // markdown file is highlighted; "Open file" / "Open directory" follow.
    if let Some(filename) = &targets.pdf {
        v.push((Choice::CreatePdf, create_pdf_label(filename)));
    }
    if let Some(filename) = &targets.open_file {
        v.push((Choice::OpenFile, open_file_label(filename)));
    }
    if let Some(rel_dir) = &targets.open_dir {
        v.push((Choice::OpenDir, open_dir_label(rel_dir)));
    }
    v.extend(
        STATIC_ITEMS
            .iter()
            .filter(|(c, _)| include_msg || *c != Choice::Msg)
            .map(|(c, l)| (*c, (*l).to_owned())),
    );
    v.push((Choice::ToggleLayout, layout_choice_label(side).to_owned()));
    // "Delete" trails the list, deliberately never the default-selected row:
    // a destructive action should not fire from a stray Enter on palette open.
    if let Some(filename) = &targets.delete {
        v.push((Choice::Delete, delete_label(filename)));
    }
    v
}

/// Direct keystroke that fires a choice without opening the palette,
/// rendered as a dim `[…]` annotation next to the palette row. `None` when a
/// row has no direct shortcut.
#[must_use]
pub const fn shortcut_for(choice: Choice) -> Option<&'static str> {
    match choice {
        Choice::CreatePdf => Some("^G"),
        // "Open file" / "Open directory" reuse the picker's existing keys:
        // plain Enter opens the file, Ctrl-Enter reveals its directory.
        Choice::OpenFile => Some("↵"),
        Choice::OpenDir => Some("^↵"),
        Choice::Delete => Some("^D"),
        Choice::Msg => Some("^M"),
        Choice::OpenTasks => Some("^T"),
        Choice::SearchProjects
        | Choice::SearchAreas
        | Choice::SearchResources
        | Choice::SearchArchive
        | Choice::GlobalSearch
        | Choice::ToggleLayout => None,
    }
}

/// The string a row is matched against: its 1-based number plus its label,
/// e.g. `"6. Search resources"`. Including the number makes both `6` and any
/// label word find the row.
fn matchable_text(index: usize, label: &str) -> String {
    format!("{}. {label}", index + 1)
}

/// Substring filter mirroring the picker's word-atom semantics: every
/// whitespace-separated word in `query` must appear (case-insensitively)
/// somewhere in the row's `matchable_text`. An empty query matches all rows.
fn item_matches(query: &str, index: usize, label: &str) -> bool {
    let haystack = matchable_text(index, label).to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

/// Indices into `rows` that match `query`, in menu order.
fn filter_indices(rows: &[(Choice, String)], query: &str) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(i, (_, label))| item_matches(query, *i, label))
        .map(|(i, _)| i)
        .collect()
}

/// Menu state. The list is filtered by `query`; `selected` indexes into the
/// filtered view, never into `rows` directly. Navigation and filtering are
/// pure methods so they're unit-testable without a TUI.
pub struct MenuApp {
    query: String,
    /// The ordered rows, built for the current panel side at open time.
    rows: Vec<(Choice, String)>,
    /// Indices into `rows` that match the current query, in menu order.
    filtered: Vec<usize>,
    /// Index into `filtered` of the highlighted row.
    selected: usize,
}

impl MenuApp {
    /// Open the palette for the given panel side (controls the layout-toggle
    /// row's label). `include_msg` controls whether the "Message brain" row
    /// is offered (hidden when the brain panel is already open). `targets`
    /// carries the highlighted entry's contextual row text: when a field is
    /// `Some`, the corresponding row appears — the "Create PDF" / "Open file"
    /// / "Open directory" rows lead the list, and "Delete" trails it.
    #[must_use]
    pub fn new(side: PanelSide, include_msg: bool, targets: &Targets) -> Self {
        let mut app = Self {
            query: String::new(),
            rows: items(side, include_msg, targets),
            filtered: Vec::new(),
            selected: 0,
        };
        app.refilter();
        app
    }

    fn refilter(&mut self) {
        self.filtered = filter_indices(&self.rows, &self.query);
        self.selected = 0;
    }

    const fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    /// The `Choice` under the cursor, or `None` when nothing matches.
    fn selected_choice(&self) -> Option<Choice> {
        self.filtered.get(self.selected).map(|&i| self.rows[i].0)
    }
}

/// What a keypress asks the menu loop to do next. Split out as a pure
/// function (`handle_key`) so navigation is unit-testable without a TUI.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep looping.
    Continue,
    /// Confirm this choice.
    Confirm(Choice),
    /// Esc / Ctrl-c: close the palette with no choice.
    Cancel,
}

/// Pure key handling. Movement saturates at the ends; printable chars (and
/// Backspace / Ctrl-u / Ctrl-w) edit the query and refilter; Enter confirms
/// the highlighted row; Esc / Ctrl-c cancel.
pub fn handle_key(app: &mut MenuApp, k: KeyEvent) -> Step {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);

    match k.code {
        KeyCode::Esc => return Step::Cancel,
        KeyCode::Char('c') if ctrl => return Step::Cancel,
        KeyCode::Enter => {
            if let Some(choice) = app.selected_choice() {
                return Step::Confirm(choice);
            }
        }

        KeyCode::Up => app.move_up(),
        KeyCode::Char('p' | 'k') if ctrl => app.move_up(),
        KeyCode::Down => app.move_down(),
        KeyCode::Char('n' | 'j') if ctrl => app.move_down(),

        KeyCode::Backspace => {
            app.query.pop();
            app.refilter();
        }
        KeyCode::Char('u') if ctrl => {
            app.query.clear();
            app.refilter();
        }
        KeyCode::Char('w') if ctrl => {
            let cut = app
                .query
                .trim_end()
                .rfind(char::is_whitespace)
                .map_or(0, |i| i + 1);
            app.query.truncate(cut);
            app.refilter();
        }

        KeyCode::Char(c)
            if !k
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.query.push(c);
            app.refilter();
        }
        _ => {}
    }
    Step::Continue
}

/// Columns the palette modal needs so its widest row renders — label **and**
/// shortcut hint — without being clipped by the border. Mirrors the fixed
/// decorations in [`item_line`]: 2 leading spaces, the right-aligned number
/// prefix (digits + `.`), the 3-column arrow gutter, the label, then the
/// `  [key]` hint. Floored so the `type filter …` footer always fits, and
/// padded on the right so long rows don't butt against the border.
fn palette_width(rows: &[(Choice, String)]) -> usize {
    const LEAD: usize = 2; // the two leading spaces
    const ARROW: usize = 3; // " ❯ " / "   "
    const HINT_FRAME: usize = 4; // "  [" + "]"
    const BORDERS: usize = 2;
    const RIGHT_PAD: usize = 2;
    const FOOTER_MIN: usize = 42; // the footer's " type filter … Esc back" width
    let num = rows.len().to_string().len() + 1; // right-aligned digits + '.'
    let content = rows
        .iter()
        .map(|(choice, label)| {
            let hint = shortcut_for(*choice).map_or(0, |k| HINT_FRAME + k.chars().count());
            LEAD + num + ARROW + label.chars().count() + hint
        })
        .max()
        .unwrap_or(0)
        .max(FOOTER_MIN);
    content + BORDERS + RIGHT_PAD
}

/// Render the command palette as a centered modal overlay.
///
/// Drawn on top of whatever the host already rendered; `Clear` wipes the box
/// region first so the content behind doesn't bleed through.
pub fn draw_modal(f: &mut Frame, app: &MenuApp, area: Rect) {
    // Tall enough for every row plus chrome (border 2 + input + separator +
    // footer = 5), clamped to the screen.
    let rows = u16::try_from(app.filtered.len().max(1)).unwrap_or(u16::MAX);
    let height = rows.saturating_add(5).min(area.height);
    let width = u16::try_from(palette_width(&app.rows))
        .unwrap_or(u16::MAX)
        .min(area.width);
    let modal = centered_rect(width, height, area);

    f.render_widget(Clear, modal);
    let accent = Style::new()
        .fg(render::ACCENT_PURPLE)
        .add_modifier(Modifier::BOLD);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(render::ACCENT_PURPLE))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("Command palette", accent),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // input
            Constraint::Length(1), // separator
            Constraint::Min(1),    // list
            Constraint::Length(1), // footer
        ])
        .split(inner);

    f.render_widget(Paragraph::new(render::input_line(&app.query)), chunks[0]);
    f.render_widget(
        Paragraph::new(render::separator_line(inner.width as usize)),
        chunks[1],
    );

    if app.filtered.is_empty() {
        f.render_widget(Paragraph::new(render::empty_line(false)), chunks[2]);
    } else {
        let item_lines: Vec<Line<'static>> = app
            .filtered
            .iter()
            .enumerate()
            .map(|(row, &item_idx)| {
                let (choice, label) = &app.rows[item_idx];
                item_line(
                    item_idx,
                    app.rows.len(),
                    label,
                    row == app.selected,
                    shortcut_for(*choice),
                )
            })
            .collect();
        f.render_widget(Paragraph::new(item_lines), chunks[2]);
    }

    let key = Style::new()
        .fg(render::TEXT_PRIMARY)
        .add_modifier(Modifier::BOLD);
    let lbl = Style::new().fg(render::TEXT_DIM);
    let dot = Style::new().fg(render::TEXT_VERY_DIM);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled("type", key),
            Span::styled(" filter", lbl),
            Span::styled("  ", dot),
            Span::styled("↑↓", key),
            Span::styled(" move", lbl),
            Span::styled("  ", dot),
            Span::styled("Enter", key),
            Span::styled(" run", lbl),
            Span::styled("  ", dot),
            Span::styled("Esc", key),
            Span::styled(" back", lbl),
        ])),
        chunks[3],
    );
}

/// A `width`×`height` rectangle centered within `area` (clamped to it).
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// The `"N."` prefix for a row, right-aligned to the width of the largest
/// number in a `total`-row palette. Once the palette hits double digits, the
/// single-digit rows gain a leading space so every dot lines up.
fn number_prefix(index: usize, total: usize) -> String {
    let width = total.to_string().len();
    format!("{:>width$}.", index + 1)
}

fn item_line(
    index: usize,
    total: usize,
    label: &str,
    selected: bool,
    shortcut: Option<&'static str>,
) -> Line<'static> {
    let arrow = if selected { " ❯ " } else { "   " };
    let (num_style, arrow_style, label_style) = if selected {
        (
            Style::new()
                .fg(render::ACCENT_CYAN)
                .bg(render::SELECTED_BG)
                .add_modifier(Modifier::BOLD),
            Style::new()
                .fg(render::ACCENT_CYAN)
                .bg(render::SELECTED_BG)
                .add_modifier(Modifier::BOLD),
            Style::new()
                .fg(render::TEXT_PRIMARY)
                .bg(render::SELECTED_BG)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::new().fg(render::TEXT_VERY_DIM),
            Style::new().fg(render::TEXT_VERY_DIM),
            Style::new().fg(render::TEXT_DIM),
        )
    };
    let mut spans = vec![
        Span::styled("  ", num_style),
        Span::styled(number_prefix(index, total), num_style),
        Span::styled(arrow.to_owned(), arrow_style),
        Span::styled(label.to_owned(), label_style),
    ];
    // The shortcut hint stays dim regardless of selection — it's metadata,
    // not part of the focused-row emphasis.
    if let Some(key) = shortcut {
        spans.push(Span::styled(
            format!("  [{key}]"),
            Style::new().fg(render::TEXT_VERY_DIM),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn rows() -> Vec<(Choice, String)> {
        items(PanelSide::Right, true, &Targets::default())
    }

    /// A `Targets` with just the PDF field set (the common single-row case).
    fn pdf_target(name: &str) -> Targets {
        Targets {
            pdf: Some(name.to_owned()),
            ..Targets::default()
        }
    }

    #[test]
    fn message_brain_is_hidden_when_the_panel_is_open() {
        // include_msg = false → the brain panel is already open, so the
        // "Message brain" row is dropped (you can't re-open what's open).
        let closed = items(PanelSide::Right, true, &Targets::default());
        let open = items(PanelSide::Right, false, &Targets::default());
        assert!(closed.iter().any(|(c, _)| *c == Choice::Msg));
        assert!(!open.iter().any(|(c, _)| *c == Choice::Msg));
        assert_eq!(open.len(), closed.len() - 1);
    }

    // --- the contextual "Create PDF" row --------------------------------

    #[test]
    fn create_pdf_row_appears_only_with_a_markdown_target() {
        let without = items(PanelSide::Right, true, &Targets::default());
        let with = items(PanelSide::Right, true, &pdf_target("plan.md"));
        assert!(!without.iter().any(|(c, _)| *c == Choice::CreatePdf));
        assert_eq!(with.len(), without.len() + 1);
        // It leads the list so it's the default-selected action on open.
        assert_eq!(with[0].0, Choice::CreatePdf);
        assert_eq!(with[0].1, "Create PDF for 'plan.md'");
    }

    // --- the contextual "Open file" / "Open directory" rows -------------

    #[test]
    fn open_file_row_appears_only_with_a_file_target_and_leads() {
        let without = items(PanelSide::Right, true, &Targets::default());
        let with = items(
            PanelSide::Right,
            true,
            &Targets {
                open_file: Some("note.md".to_owned()),
                ..Targets::default()
            },
        );
        assert!(!without.iter().any(|(c, _)| *c == Choice::OpenFile));
        assert_eq!(with.len(), without.len() + 1);
        // No PDF target, so "Open file" leads (the default-selected action).
        assert_eq!(with[0].0, Choice::OpenFile);
        assert_eq!(with[0].1, "Open file 'note.md'");
    }

    #[test]
    fn open_dir_row_appears_only_with_a_dir_target_and_leads() {
        let without = items(PanelSide::Right, true, &Targets::default());
        let with = items(
            PanelSide::Right,
            true,
            &Targets {
                open_dir: Some("projects/foo".to_owned()),
                ..Targets::default()
            },
        );
        assert!(!without.iter().any(|(c, _)| *c == Choice::OpenDir));
        assert_eq!(with.len(), without.len() + 1);
        assert_eq!(with[0].0, Choice::OpenDir);
        assert_eq!(with[0].1, "Open dir 'projects/foo'");
    }

    #[test]
    fn contextual_rows_order_pdf_then_open_file_then_open_dir() {
        // All three entry-action rows lead the list, in this fixed order.
        let all = items(
            PanelSide::Right,
            true,
            &Targets {
                pdf: Some("plan.md".to_owned()),
                open_file: Some("plan.md".to_owned()),
                open_dir: Some("projects/foo".to_owned()),
                delete: Some("plan.md".to_owned()),
            },
        );
        assert_eq!(all[0].0, Choice::CreatePdf);
        assert_eq!(all[1].0, Choice::OpenFile);
        assert_eq!(all[2].0, Choice::OpenDir);
        // Delete still trails, never leads.
        assert_eq!(all.last().unwrap().0, Choice::Delete);
    }

    #[test]
    fn open_file_and_open_dir_carry_the_enter_shortcuts() {
        // They surface the picker's existing keys, not new ones.
        assert_eq!(shortcut_for(Choice::OpenFile), Some("↵"));
        assert_eq!(shortcut_for(Choice::OpenDir), Some("^↵"));
    }

    #[test]
    fn open_file_label_elides_long_names_like_create_pdf() {
        let label = open_file_label("really-long-note-name-that-overflows.md");
        assert!(label.starts_with("Open file 'really-long"), "got: {label}");
        assert!(label.contains("..."), "got: {label}");
        let shown = label
            .trim_start_matches("Open file '")
            .trim_end_matches('\'');
        assert_eq!(shown.chars().count(), LABEL_MAX_FILENAME);
    }

    #[test]
    fn open_dir_label_shows_a_short_path_in_full() {
        assert_eq!(
            open_dir_label("projects/foo/bar"),
            "Open dir 'projects/foo/bar'"
        );
    }

    #[test]
    fn open_dir_label_keeps_category_and_middle_elides_a_long_path() {
        // Over the threshold: the leading category survives, the middle is
        // dropped, and the trailing chars fill the remaining budget.
        let rel = "resources/aa/bb/cc/dd/final/parts/of/path";
        let label = open_dir_label(rel);
        let shown = label
            .trim_start_matches("Open dir '")
            .trim_end_matches('\'')
            .to_owned();
        assert!(shown.starts_with("resources/..."), "got: {shown}");
        assert!(shown.ends_with("path"), "got: {shown}");
        assert_eq!(shown.chars().count(), LABEL_MAX_DIR);
    }

    #[test]
    fn open_dir_label_uses_the_short_open_dir_prefix() {
        assert!(
            open_dir_label("projects/foo").starts_with("Open dir '"),
            "the row is labeled 'Open dir', not 'Open directory'"
        );
    }

    #[test]
    fn open_dir_label_budget_is_wider_than_a_filename() {
        // The directory path gets more room (26) than an elided filename (24).
        assert_eq!(LABEL_MAX_DIR, 26);
        let rel = "resources/aa/bb/cc/dd/eee/final/parts/of/path";
        let shown = open_dir_label(rel)
            .trim_start_matches("Open dir '")
            .trim_end_matches('\'')
            .to_owned();
        assert_eq!(shown.chars().count(), LABEL_MAX_DIR);
    }

    #[test]
    fn create_pdf_row_carries_the_ctrl_g_shortcut() {
        assert_eq!(shortcut_for(Choice::CreatePdf), Some("^G"));
    }

    #[test]
    fn palette_width_fits_the_widest_row_including_its_shortcut() {
        // A contextual "Open directory" row elided to the label budget, plus
        // its `[^↵]` hint, is about the widest thing the palette draws. The
        // modal must be wide enough to show that hint without clipping it at
        // the border (the bug in the screenshot).
        let rows = vec![
            (
                Choice::OpenDir,
                open_dir_label("projects/personal__foo/docs/integrations"),
            ),
            (Choice::SearchProjects, "Search projects".to_owned()),
        ];
        let width = palette_width(&rows);

        // Reconstruct the widest row's rendered column count: 2 leading spaces
        // + right-aligned number prefix + 3-col arrow gutter + label + hint.
        let num = rows.len().to_string().len() + 1;
        let (choice, label) = &rows[0];
        let hint = 4 + shortcut_for(*choice).unwrap().chars().count();
        let row = 2 + num + 3 + label.chars().count() + hint;

        assert!(
            width >= row + 2, // + the two side borders
            "modal width {width} must fit row {row} plus its borders"
        );
    }

    #[test]
    fn short_filenames_are_shown_in_full() {
        assert_eq!(create_pdf_label("plan.md"), "Create PDF for 'plan.md'");
    }

    #[test]
    fn long_filenames_are_elided_with_a_trailing_md() {
        let label = create_pdf_label("really-long-note-name-that-overflows.md");
        assert!(label.starts_with("Create PDF for 'really-long"), "got: {label}");
        assert!(label.contains("..."), "got: {label}");
        assert!(label.ends_with("md'"), "got: {label}");
        // The shown filename is capped at LABEL_MAX_FILENAME chars.
        let shown = label
            .trim_start_matches("Create PDF for '")
            .trim_end_matches('\'');
        assert_eq!(shown.chars().count(), LABEL_MAX_FILENAME);
    }

    #[test]
    fn truncation_keeps_head_ellipsis_and_two_tail_chars() {
        // Deterministic shape: 19-char head + "..." + "md" = 24.
        assert_eq!(
            truncate_label_filename("abcdefghijklmnopqrstuvwxyz.md", 24),
            "abcdefghijklmnopqrs...md"
        );
    }

    #[test]
    fn truncation_always_keeps_the_full_extension() {
        // A 3-char extension survives whole — `mp4`, never `p4`.
        let label = truncate_label_filename("a-really-long-clip-name-here.mp4", 24);
        assert!(label.ends_with("...mp4"), "got: {label}");
        assert_eq!(label.chars().count(), 24);

        // A longer extension (`webp`) is still shown in full.
        let webp = truncate_label_filename("some-long-screenshot-name.webp", 24);
        assert!(webp.ends_with("...webp"), "got: {webp}");

        // No extension → the previous two-char tail behavior.
        let bare = truncate_label_filename("a-really-long-name-without-ext", 24);
        assert!(bare.contains("..."), "got: {bare}");
        assert_eq!(bare.chars().count(), 24);
    }

    #[test]
    fn delete_row_appears_only_with_a_target_and_trails_the_list() {
        let without = items(PanelSide::Right, true, &Targets::default());
        let with = items(
            PanelSide::Right,
            true,
            &Targets {
                delete: Some("old.md".to_owned()),
                ..Targets::default()
            },
        );
        assert!(!without.iter().any(|(c, _)| *c == Choice::Delete));
        assert_eq!(with.len(), without.len() + 1);
        // It trails the list so a stray Enter on palette open can't delete.
        assert_eq!(with.last().unwrap().0, Choice::Delete);
        assert_eq!(with.last().unwrap().1, "Delete 'old.md'");
        assert_ne!(with[0].0, Choice::Delete);
    }

    #[test]
    fn delete_row_carries_the_ctrl_d_shortcut() {
        assert_eq!(shortcut_for(Choice::Delete), Some("^D"));
    }

    #[test]
    fn delete_label_shares_the_create_pdf_ellipsis_threshold() {
        let name = "really-long-note-name-that-overflows.md";
        let shown = delete_label(name)
            .trim_start_matches("Delete '")
            .trim_end_matches('\'')
            .to_owned();
        assert!(shown.contains("..."), "got: {shown}");
        // Same cap as the Create PDF row (LABEL_MAX_FILENAME).
        assert_eq!(shown.chars().count(), LABEL_MAX_FILENAME);
    }

    #[test]
    fn menu_rows_are_in_the_expected_order() {
        let order: Vec<Choice> = rows().iter().map(|(c, _)| *c).collect();
        assert_eq!(
            order,
            vec![
                Choice::Msg,
                Choice::OpenTasks,
                Choice::SearchProjects,
                Choice::SearchAreas,
                Choice::SearchResources,
                Choice::SearchArchive,
                Choice::GlobalSearch,
                Choice::ToggleLayout,
            ]
        );
    }

    #[test]
    fn toggle_layout_is_the_last_row_and_names_the_opposite_side() {
        let r = rows();
        let last = r.last().expect("menu is non-empty");
        assert_eq!(last.0, Choice::ToggleLayout);
        // Panel on the right → offer to move it left.
        assert_eq!(last.1, "Move brain panel to the left");
        // And vice versa.
        assert_eq!(
            layout_choice_label(PanelSide::Left),
            "Move brain panel to the right"
        );
    }

    #[test]
    fn msg_row_is_labeled_message_brain() {
        let r = rows();
        let msg = r
            .iter()
            .find(|(c, _)| *c == Choice::Msg)
            .expect("Msg row exists");
        assert_eq!(msg.1, "Message brain");
    }

    #[test]
    fn every_choice_appears_exactly_once() {
        // Guards against a Choice variant being added without a menu row.
        // CreatePdf is conditional (only with a markdown target), so it's
        // checked separately below; the rest must always appear exactly once.
        let all = [
            Choice::Msg,
            Choice::OpenTasks,
            Choice::SearchProjects,
            Choice::SearchAreas,
            Choice::SearchResources,
            Choice::SearchArchive,
            Choice::GlobalSearch,
            Choice::ToggleLayout,
        ];
        let r = rows();
        assert_eq!(r.len(), all.len());
        for choice in all {
            let count = r.iter().filter(|(c, _)| *c == choice).count();
            assert_eq!(count, 1, "{choice:?} should appear exactly once");
        }
        // With a markdown target, CreatePdf appears exactly once and every
        // other choice still appears exactly once.
        let with_pdf = items(PanelSide::Right, true, &pdf_target("plan.md"));
        assert_eq!(with_pdf.len(), all.len() + 1);
        for choice in all.iter().chain(std::iter::once(&Choice::CreatePdf)) {
            let count = with_pdf.iter().filter(|(c, _)| c == choice).count();
            assert_eq!(count, 1, "{choice:?} should appear exactly once");
        }
    }

    #[test]
    fn only_msg_and_tasks_carry_shortcuts() {
        assert_eq!(shortcut_for(Choice::Msg), Some("^M"));
        assert_eq!(shortcut_for(Choice::OpenTasks), Some("^T"));
        assert_eq!(shortcut_for(Choice::SearchProjects), None);
        assert_eq!(shortcut_for(Choice::SearchArchive), None);
        assert_eq!(shortcut_for(Choice::GlobalSearch), None);
        assert_eq!(shortcut_for(Choice::ToggleLayout), None);
    }

    #[test]
    fn shortcut_hint_is_rendered_dim_next_to_its_row() {
        // Row 0 is "Message brain" → carries the ^M hint.
        let line = item_line(0, 9, "Message brain", false, shortcut_for(Choice::Msg));
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("[^M]"), "got: {text}");
        let hint = line
            .spans
            .iter()
            .find(|s| s.content.contains("^M"))
            .expect("the ^M hint span exists");
        assert_eq!(hint.style.fg, Some(render::TEXT_VERY_DIM));
    }

    // --- number prefix alignment ----------------------------------------

    #[test]
    fn single_digit_palette_has_no_padding() {
        // 9 or fewer rows: every number is one digit, so no leading space.
        assert_eq!(number_prefix(0, 9), "1.");
        assert_eq!(number_prefix(8, 9), "9.");
    }

    #[test]
    fn double_digit_palette_pads_single_digit_numbers() {
        // 10+ rows: single-digit numbers gain a leading space so the dots
        // (and everything after them) line up with the two-digit rows.
        assert_eq!(number_prefix(0, 12), " 1.");
        assert_eq!(number_prefix(8, 12), " 9.");
        assert_eq!(number_prefix(9, 12), "10.");
        assert_eq!(number_prefix(11, 12), "12.");
    }

    // --- matchable text / filtering -------------------------------------

    #[test]
    fn matchable_text_includes_the_one_based_number() {
        assert_eq!(matchable_text(0, "Message brain"), "1. Message brain");
        assert_eq!(matchable_text(7, "Global search"), "8. Global search");
    }

    #[test]
    fn empty_query_matches_every_row() {
        let r = rows();
        assert_eq!(filter_indices(&r, ""), (0..r.len()).collect::<Vec<_>>());
    }

    #[test]
    fn digit_query_matches_the_row_with_that_number() {
        let r = rows();
        // "7" is matchable because the number is part of the row's text.
        // Row 7 is "Global search" now that the dropped "Go to root" row no
        // longer sits between "Open tasks" and the searches.
        let hits = filter_indices(&r, "7");
        assert_eq!(hits, vec![6]);
        assert_eq!(r[hits[0]].0, Choice::GlobalSearch);
    }

    #[test]
    fn archive_row_is_searchable_by_label() {
        let r = rows();
        let hits = filter_indices(&r, "archive");
        assert_eq!(hits.len(), 1);
        assert_eq!(r[hits[0]].0, Choice::SearchArchive);
    }

    #[test]
    fn layout_row_is_searchable_by_label() {
        let r = rows();
        let hits = filter_indices(&r, "move brain panel");
        assert_eq!(hits.len(), 1);
        assert_eq!(r[hits[0]].0, Choice::ToggleLayout);
    }

    #[test]
    fn query_is_case_insensitive() {
        let r = rows();
        assert_eq!(filter_indices(&r, "MESSAGE"), filter_indices(&r, "message"));
        assert!(!filter_indices(&r, "MESSAGE").is_empty());
    }

    #[test]
    fn every_word_must_match() {
        let r = rows();
        // "search projects" both appear only in the Search projects row.
        let hits = filter_indices(&r, "search projects");
        assert_eq!(hits.len(), 1);
        assert_eq!(r[hits[0]].0, Choice::SearchProjects);
    }

    #[test]
    fn unmatched_query_yields_no_rows() {
        let r = rows();
        assert!(filter_indices(&r, "nonexistentxyz").is_empty());
    }

    // --- MenuApp navigation ---------------------------------------------

    #[test]
    fn new_app_selects_first_row_with_full_list() {
        let app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        assert_eq!(app.filtered.len(), app.rows.len());
        assert_eq!(app.selected, 0);
        assert_eq!(app.selected_choice(), Some(Choice::Msg));
    }

    #[test]
    fn down_moves_toward_the_end_and_saturates() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        app.move_down();
        assert_eq!(app.selected, 1);
        for _ in 0..50 {
            app.move_down();
        }
        assert_eq!(app.selected, app.filtered.len() - 1);
    }

    #[test]
    fn up_saturates_at_zero() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        app.move_up();
        assert_eq!(app.selected, 0);
        app.move_down();
        app.move_down();
        app.move_up();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn filtering_resets_selection_and_tracks_filtered_view() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        app.move_down();
        app.move_down();
        for c in "tasks".chars() {
            handle_key(&mut app, key(KeyCode::Char(c)));
        }
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.selected, 0);
        assert_eq!(app.selected_choice(), Some(Choice::OpenTasks));
    }

    // --- handle_key -----------------------------------------------------

    #[test]
    fn typing_filters_and_backspace_restores() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        handle_key(&mut app, key(KeyCode::Char('7')));
        assert_eq!(app.query, "7");
        assert_eq!(app.selected_choice(), Some(Choice::GlobalSearch));
        handle_key(&mut app, key(KeyCode::Backspace));
        assert_eq!(app.query, "");
        assert_eq!(app.filtered.len(), app.rows.len());
    }

    #[test]
    fn ctrl_u_clears_the_query() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        for c in "search".chars() {
            handle_key(&mut app, key(KeyCode::Char(c)));
        }
        handle_key(&mut app, ctrl_key(KeyCode::Char('u')));
        assert_eq!(app.query, "");
        assert_eq!(app.filtered.len(), app.rows.len());
    }

    #[test]
    fn ctrl_w_deletes_the_last_word() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        for c in "search projects".chars() {
            handle_key(&mut app, key(KeyCode::Char(c)));
        }
        handle_key(&mut app, ctrl_key(KeyCode::Char('w')));
        assert_eq!(app.query, "search ");
    }

    #[test]
    fn ctrl_jk_mirror_arrows_over_the_filtered_list() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        handle_key(&mut app, ctrl_key(KeyCode::Char('j')));
        assert_eq!(app.selected, 1);
        handle_key(&mut app, ctrl_key(KeyCode::Char('k')));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn plain_jk_are_query_input_not_navigation() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        handle_key(&mut app, key(KeyCode::Char('z')));
        assert_eq!(app.query, "z");
        assert!(app.filtered.is_empty());
        app.query.clear();
        app.refilter();
        handle_key(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.query, "j");
    }

    #[test]
    fn enter_confirms_the_highlighted_row() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        handle_key(&mut app, key(KeyCode::Down));
        // Row 1 (0-based) is Open tasks.
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter)),
            Step::Confirm(Choice::OpenTasks)
        );
    }

    #[test]
    fn enter_with_no_matches_keeps_looping() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        for c in "zzz".chars() {
            handle_key(&mut app, key(KeyCode::Char(c)));
        }
        assert!(app.filtered.is_empty());
        assert_eq!(handle_key(&mut app, key(KeyCode::Enter)), Step::Continue);
    }

    #[test]
    fn esc_and_ctrl_c_cancel() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        assert_eq!(handle_key(&mut app, key(KeyCode::Esc)), Step::Cancel);
        assert_eq!(
            handle_key(&mut app, ctrl_key(KeyCode::Char('c'))),
            Step::Cancel
        );
    }
}
