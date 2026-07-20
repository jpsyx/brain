//! Ratatui-based fuzzy picker.
//!
//! Returns the selected absolute path on Enter, or `Ok(None)` on Esc / Ctrl-c.
//!
//! Rendering uses `/dev/tty` instead of stdout so the calling wrapper can
//! capture this binary's stdout (the shell-side plan) without garbling the
//! TUI. crossterm's raw-mode toggles + event reader operate on the
//! controlling terminal directly, so they're unaffected.
//!
//! Matching is delegated to `nucleo-matcher` using substring atoms: every
//! whitespace-separated word in the query must appear as a contiguous run
//! of characters in the haystack. Before matching, each entry's
//! `~/brain/...` display string is normalized by dropping slug separators
//! (`-`, `_`, `.`) so a slug like `ann-afloat` is matched as `annafloat`
//! and both `annafloat` and `ann afloat` find it. Highlight indices nucleo
//! returns against the normalized string are mapped back to byte offsets
//! in the original display for rendering.
//!
//! Matches are grouped by `Bucket` (Projects → Areas → Resources) with a
//! section header per group. Headers occupy a display row but are not
//! selectable; the `selected` cursor only walks match indices.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{
        self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Paragraph,
};

use crate::confirm::{self, Confirm, ConfirmKind};
use crate::entry::{Bucket, Entry};
use crate::menu::{self, Choice};
use crate::open_target;
use crate::render;

/// What the picker should do with the selected path. Driven by which Enter
/// variant the user pressed.
#[derive(Debug)]
pub enum Selection {
    /// Ctrl-Enter: reveal the path in Finder (files resolve to their parent
    /// dir at the call site).
    Reveal(PathBuf),
    /// Plain Enter: open the path directly (editor for text-like files,
    /// system `open` for everything else; dirs still land in Finder).
    Open(PathBuf),
}

/// How the picker exited: the user chose a path, or confirmed a
/// command-palette `Choice`.
///
/// Opening the palette and dismissing it with Esc never exits the picker —
/// that's handled inside the event loop as a modal overlay.
#[derive(Debug)]
pub enum Outcome {
    /// A path was picked (plain Enter or Ctrl-Enter).
    Selected(Selection),
    /// A command-palette row was confirmed with Enter.
    Choice(Choice),
    /// The user confirmed converting this markdown file to a PDF (via the
    /// `Ctrl-G` confirmation modal or the palette's "Create PDF" row).
    CreatePdf(PathBuf),
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct Match {
    entry_idx: usize,
    bucket: Bucket,
    score: u32,
    /// Byte offsets into `Entry::display` for highlighting. Empty when the
    /// query is empty (everything is shown unfiltered).
    highlight_bytes: BTreeSet<usize>,
}

/// One row in the rendered list. Selection only ever lands on `Match`.
#[derive(Copy, Clone, Debug)]
enum DisplayRow {
    /// Section heading: bucket + how many matches in that section.
    Header(Bucket, usize),
    /// Index into `App::matches`.
    Match(usize),
}

pub struct App {
    /// Owned so the persistent two-panel TUI can rescope the search to a
    /// different bucket set in place (`set_entries`). The one-shot picker
    /// clones the caller's slice once at startup.
    entries: Vec<Entry>,
    /// `~/brain/...` display strings precomputed as `Utf32String` buffers
    /// for nucleo. Same indexing as `entries`.
    haystacks: Vec<HaystackBuf>,
    matcher: Matcher,
    pub(crate) query: String,
    /// All matches for the current query, sorted by bucket (P → A → R), then
    /// by score within each bucket.
    matches: Vec<Match>,
    /// Interleaved headers + matches in render order. Rebuilt with `matches`.
    display_rows: Vec<DisplayRow>,
    /// Index into `matches` of the currently-selected match.
    selected: usize,
    /// First visible display row. Kept consistent with `selected` so the
    /// cursor never scrolls off-screen and the section header above the
    /// selected match stays visible.
    top: usize,
    /// The command-palette overlay, when open (`Ctrl-p`). While `Some`, all
    /// keys route to it; Esc closes it back to the picker.
    pub(crate) palette: Option<menu::MenuApp>,
    /// The confirmation overlay, when open: "Create PDF" (`Ctrl-G` on a
    /// markdown file) or "Delete" (`Ctrl-D` on any entry). Takes routing
    /// precedence over the palette; Esc/No closes it back to the picker.
    pub(crate) confirm: Option<Confirm>,
}

/// Per-entry preprocessing for nucleo matching + highlight mapping.
struct HaystackBuf {
    /// The display string with slug separators (`-`, `_`, `.`) stripped.
    /// Nucleo matches against this, so word atoms like `afloat` find
    /// slugs like `ann-afloat` without the dashes splitting the run.
    normalized: String,
    /// For each char position in `normalized`, the byte offset of the same
    /// char in the original `Entry::display`. Built once at startup so
    /// nucleo's highlight indices (char positions in the normalized
    /// `Utf32Str`) translate cheaply to display byte offsets at render time.
    normalized_char_to_display_byte: Vec<usize>,
}

impl HaystackBuf {
    fn new(display: &str) -> Self {
        let mut normalized = String::with_capacity(display.len());
        let mut map = Vec::with_capacity(display.len());
        for (byte_idx, ch) in display.char_indices() {
            if !matches!(ch, '-' | '_' | '.') {
                normalized.push(ch);
                map.push(byte_idx);
            }
        }
        Self {
            normalized,
            normalized_char_to_display_byte: map,
        }
    }
}

impl App {
    pub(crate) fn new(entries: &[Entry], initial: &str) -> Self {
        let haystacks: Vec<HaystackBuf> = entries
            .iter()
            .map(|e| HaystackBuf::new(&e.display))
            .collect();
        let mut app = Self {
            entries: entries.to_vec(),
            haystacks,
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            query: initial.to_owned(),
            matches: Vec::new(),
            display_rows: Vec::new(),
            selected: 0,
            top: 0,
            palette: None,
            confirm: None,
        };
        app.refilter();
        app
    }

    /// Replace the searchable entry set in place and re-run the filter from a
    /// clean slate. Used by the persistent TUI's palette to rescope the
    /// search to a single bucket (or back to global) without quitting.
    pub(crate) fn set_entries(&mut self, entries: &[Entry]) {
        self.haystacks = entries.iter().map(|e| HaystackBuf::new(&e.display)).collect();
        self.entries = entries.to_vec();
        self.query.clear();
        self.refilter();
    }

    /// Replace the entry set while **keeping** the current query — a refresh
    /// in place (`Ctrl-R`, or after a PDF/delete changes the tree), as opposed
    /// to `set_entries` which clears the query for a scope switch.
    pub(crate) fn reload_entries(&mut self, entries: &[Entry]) {
        self.haystacks = entries.iter().map(|e| HaystackBuf::new(&e.display)).collect();
        self.entries = entries.to_vec();
        self.refilter();
    }

    /// Drop `path` and everything beneath it from the in-memory entry set and
    /// re-filter, keeping the query. Used by the one-shot picker (which has no
    /// roots to re-walk) to reflect a trashed file or directory immediately.
    pub(crate) fn drop_path(&mut self, path: &Path) {
        let kept: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| e.path != path && !e.path.starts_with(path))
            .cloned()
            .collect();
        self.reload_entries(&kept);
    }

    // -- query mutations (pub(crate) for the embedded search panel) -------

    pub(crate) fn push_query(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub(crate) fn pop_query(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub(crate) fn clear_query(&mut self) {
        self.query.clear();
        self.refilter();
    }

    pub(crate) fn delete_word(&mut self) {
        let cut = self
            .query
            .trim_end()
            .rfind(char::is_whitespace)
            .map_or(0, |i| i + 1);
        self.query.truncate(cut);
        self.refilter();
    }

    pub(crate) const fn jump_first(&mut self) {
        self.selected = 0;
    }

    pub(crate) fn jump_last(&mut self) {
        self.selected = self.matches.len().saturating_sub(1);
    }

    pub(crate) fn open_palette(&mut self, side: crate::state::PanelSide, include_msg: bool) {
        let targets = menu::Targets {
            pdf: self.selected_markdown_filename(),
            open_file: self.selected_file_filename(),
            open_dir: self.selected_dir_reldisplay(),
            delete: self.selected_filename(),
        };
        self.palette = Some(menu::MenuApp::new(side, include_msg, &targets));
    }

    pub(crate) fn close_palette(&mut self) {
        self.palette = None;
    }

    /// The absolute path of the highlighted entry when it is a markdown file,
    /// else `None`. Drives the contextual "Create PDF" row and `Ctrl-G`.
    pub(crate) fn selected_markdown_path(&self) -> Option<PathBuf> {
        let path = self.selected_path()?;
        open_target::is_markdown(&path).then_some(path)
    }

    /// The filename (not the full path) of the highlighted markdown entry, for
    /// the palette row label.
    pub(crate) fn selected_markdown_filename(&self) -> Option<String> {
        self.selected_markdown_path()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// The filename (not the full path) of the highlighted entry, of any kind,
    /// for the contextual "Delete '…'" palette row.
    pub(crate) fn selected_filename(&self) -> Option<String> {
        self.selected_path()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// The filename of the highlighted entry when it is a **file**, for the
    /// contextual "Open file '…'" palette row. `None` for a directory (there's
    /// no file to open) so the row is suppressed.
    pub(crate) fn selected_file_filename(&self) -> Option<String> {
        let path = self.selected_path()?;
        if !path.is_file() {
            return None;
        }
        path.file_name().map(|n| n.to_string_lossy().into_owned())
    }

    /// The highlighted entry's directory as a bucket-relative display path
    /// (e.g. `projects/foo`), for the contextual "Open directory '…'" palette
    /// row. A file resolves to its parent directory; a directory resolves to
    /// itself (mirroring `open_target::finder_target`).
    pub(crate) fn selected_dir_reldisplay(&self) -> Option<String> {
        let m = self.matches.get(self.selected)?;
        let entry = &self.entries[m.entry_idx];
        let category = entry.bucket.label().to_ascii_lowercase();
        let rel = bucket_relative(&entry.display, &category)?;
        Some(if entry.path.is_dir() {
            rel
        } else {
            parent_reldisplay(&rel)
        })
    }

    /// Open the "Create PDF" confirmation overlay for `path`.
    pub(crate) fn open_confirm(&mut self, path: PathBuf) {
        self.confirm = Some(Confirm::pdf(path));
    }

    /// Open the (red) "Delete" confirmation overlay for `path`.
    pub(crate) fn open_delete_confirm(&mut self, path: PathBuf) {
        self.confirm = Some(Confirm::delete(path));
    }

    pub(crate) fn close_confirm(&mut self) {
        self.confirm = None;
    }

    fn refilter(&mut self) {
        let mut scored: Vec<Match> = if self.query.is_empty() {
            self.entries
                .iter()
                .enumerate()
                .map(|(i, e)| Match {
                    entry_idx: i,
                    bucket: e.bucket,
                    score: 0,
                    highlight_bytes: BTreeSet::new(),
                })
                .collect()
        } else {
            let pattern = Pattern::new(
                &self.query,
                CaseMatching::Smart,
                Normalization::Smart,
                AtomKind::Substring,
            );
            let mut out: Vec<Match> = Vec::with_capacity(self.entries.len());
            let mut haystack_buf: Vec<char> = Vec::new();
            let mut index_buf: Vec<u32> = Vec::new();
            for (i, entry) in self.entries.iter().enumerate() {
                haystack_buf.clear();
                index_buf.clear();
                let haystack = Utf32Str::new(&self.haystacks[i].normalized, &mut haystack_buf);
                if let Some(score) = pattern.indices(haystack, &mut self.matcher, &mut index_buf) {
                    let highlight_bytes = char_positions_to_byte_positions(
                        &index_buf,
                        &self.haystacks[i].normalized_char_to_display_byte,
                    );
                    out.push(Match {
                        entry_idx: i,
                        bucket: entry.bucket,
                        score,
                        highlight_bytes,
                    });
                }
            }
            out
        };

        // Group by bucket (P → A → R), preserving score order within each
        // group. For empty query, ties fall back to walkdir order.
        scored.sort_by(|a, b| {
            a.bucket
                .cmp(&b.bucket)
                .then(b.score.cmp(&a.score))
                .then(a.entry_idx.cmp(&b.entry_idx))
        });
        self.matches = scored;
        self.display_rows = build_display_rows(&self.matches);
        self.selected = 0;
        self.top = 0;
    }

    const PAGE_SIZE: usize = 10;

    pub(crate) const fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub(crate) fn move_down(&mut self) {
        if self.selected + 1 < self.matches.len() {
            self.selected += 1;
        }
    }

    pub(crate) const fn page_up(&mut self) {
        self.selected = self.selected.saturating_sub(Self::PAGE_SIZE);
    }

    pub(crate) fn page_down(&mut self) {
        let max = self.matches.len().saturating_sub(1);
        self.selected = self.selected.saturating_add(Self::PAGE_SIZE).min(max);
    }

    /// Returns the display-row index of the currently-selected match.
    fn selected_row(&self) -> Option<usize> {
        self.display_rows
            .iter()
            .position(|r| matches!(r, DisplayRow::Match(i) if *i == self.selected))
    }

    fn ensure_visible(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        let Some(sel_row) = self.selected_row() else {
            return;
        };
        // If the section header sits directly above the selected match,
        // anchor the top to the header so it stays in view.
        let header_above = sel_row > 0
            && matches!(
                self.display_rows.get(sel_row - 1),
                Some(DisplayRow::Header(_, _))
            );
        let upper_anchor = if header_above { sel_row - 1 } else { sel_row };

        if upper_anchor < self.top {
            self.top = upper_anchor;
        } else if sel_row >= self.top + height {
            self.top = sel_row + 1 - height;
        }
    }

    pub(crate) fn selected_path(&self) -> Option<PathBuf> {
        self.matches
            .get(self.selected)
            .map(|m| self.entries[m.entry_idx].path.clone())
    }
}

/// Slice an entry's `~/brain/...` display path down to its bucket-relative
/// form, starting at the `category` segment (the lowercase bucket dir name),
/// e.g. `~/brain/projects/foo/note.md` + `projects` → `projects/foo/note.md`.
///
/// Matches the first path segment equal to `category` (the top-level bucket
/// dir sits right under the brain root, so a later same-named subdirectory
/// can't shadow it). `None` if the category segment isn't present.
fn bucket_relative(display: &str, category: &str) -> Option<String> {
    let idx = display.split('/').position(|seg| seg == category)?;
    Some(
        display
            .split('/')
            .skip(idx)
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Drop the last segment of a bucket-relative path to get its parent
/// directory, e.g. `projects/foo/note.md` → `projects/foo`. A single-segment
/// path (an entry directly under a bucket root) is returned unchanged — its
/// directory *is* the bucket.
fn parent_reldisplay(rel: &str) -> String {
    rel.rsplit_once('/').map_or_else(|| rel.to_owned(), |(head, _)| head.to_owned())
}

fn build_display_rows(matches: &[Match]) -> Vec<DisplayRow> {
    if matches.is_empty() {
        return Vec::new();
    }
    // Count per-bucket so headers can show "Projects · 12".
    let mut rows: Vec<DisplayRow> = Vec::with_capacity(matches.len() + 3);
    let mut i = 0;
    while i < matches.len() {
        let bucket = matches[i].bucket;
        let start = i;
        while i < matches.len() && matches[i].bucket == bucket {
            i += 1;
        }
        rows.push(DisplayRow::Header(bucket, i - start));
        for m_idx in start..i {
            rows.push(DisplayRow::Match(m_idx));
        }
    }
    rows
}

fn char_positions_to_byte_positions(
    char_positions: &[u32],
    char_to_byte: &[usize],
) -> BTreeSet<usize> {
    char_positions
        .iter()
        .filter_map(|&cp| char_to_byte.get(cp as usize).copied())
        .collect()
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(entries: &[Entry], initial_query: &str) -> Result<Option<Outcome>> {
    // Open /dev/tty for read + write so the TUI is independent of stdio.
    let tty_w: File = OpenOptions::new().write(true).open("/dev/tty")?;

    enable_raw_mode()?;
    let mut backend_writer = tty_w;
    execute!(backend_writer, EnterAlternateScreen)?;

    // Kitty keyboard protocol disambiguation lets us tell Ctrl-Enter
    // apart from plain Enter. We push the flag unconditionally — the
    // escape is silently ignored on terminals that don't speak the
    // protocol, and the matching pop is then also a no-op, so there's
    // no risk of leaving the terminal in a weird state. We avoid
    // `supports_keyboard_enhancement()` because its DA1 + `CSI ? u`
    // probe can race teardown and leak `[?0u...[?...c` into the parent
    // shell on slower terminals.
    execute!(
        backend_writer,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )?;

    let backend = CrosstermBackend::new(backend_writer);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(entries, initial_query);
    let result = event_loop(&mut terminal, &mut app);

    // Always tear down, even on error.
    let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

/// What the key handler asks the event loop to do next.
#[derive(Debug)]
enum Step {
    /// Keep looping.
    Continue,
    /// Exit with the given result.
    Quit(Option<Outcome>),
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<Option<Outcome>> {
    loop {
        terminal.draw(|f| draw(f, app))?;

        // Poll so resize redraws stay responsive even without keypresses.
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        // Resize events fall through to the next draw on the next loop tick.
        let Event::Key(k) = event::read()? else {
            continue;
        };
        if k.kind != KeyEventKind::Press && k.kind != KeyEventKind::Repeat {
            continue;
        }
        if let Step::Quit(out) = handle_key(app, k) {
            return Ok(out);
        }
    }
}

/// Route a key to the confirmation modal when one is open. Returns `Some` with
/// the resulting `Step` (the modal owns the key), or `None` when no modal is
/// open so the caller keeps routing.
fn route_confirm_modal(app: &mut App, k: crossterm::event::KeyEvent) -> Option<Step> {
    let c = app.confirm.as_mut()?;
    Some(match confirm::handle_key(c, k) {
        confirm::Step::Continue => Step::Continue,
        confirm::Step::Cancel => {
            app.confirm = None;
            Step::Continue
        }
        confirm::Step::Accept => {
            let c = app.confirm.take().expect("confirm is Some");
            match c.kind {
                ConfirmKind::Pdf => Step::Quit(Some(Outcome::CreatePdf(c.path))),
                // Delete happens in place (move to Trash), then the picker
                // drops the trashed entry and stays open.
                ConfirmKind::Delete => {
                    let _ = open_target::move_to_trash(&c.path);
                    app.drop_path(&c.path);
                    Step::Continue
                }
            }
        }
    })
}

/// Route a key to the command-palette overlay when it's open. Returns `Some`
/// with the resulting `Step`, or `None` when the palette is closed.
fn route_palette_modal(app: &mut App, k: crossterm::event::KeyEvent) -> Option<Step> {
    let palette = app.palette.as_mut()?;
    Some(match menu::handle_key(palette, k) {
        menu::Step::Continue => Step::Continue,
        menu::Step::Cancel => {
            app.palette = None;
            Step::Continue
        }
        // "Create PDF" resolves to the highlighted markdown path (the palette
        // row is a deliberate pick, so no extra confirmation).
        menu::Step::Confirm(Choice::CreatePdf) => {
            app.palette = None;
            app.selected_markdown_path()
                .map_or(Step::Continue, |path| Step::Quit(Some(Outcome::CreatePdf(path))))
        }
        // "Open file" / "Open directory" mirror plain Enter / Ctrl-Enter:
        // open the highlighted file, or reveal its directory in Finder.
        menu::Step::Confirm(Choice::OpenFile) => {
            app.palette = None;
            app.selected_path().map_or(Step::Continue, |path| {
                Step::Quit(Some(Outcome::Selected(Selection::Open(path))))
            })
        }
        menu::Step::Confirm(Choice::OpenDir) => {
            app.palette = None;
            app.selected_path().map_or(Step::Continue, |path| {
                Step::Quit(Some(Outcome::Selected(Selection::Reveal(path))))
            })
        }
        // "Delete" always routes through the red confirmation modal, even from
        // the palette — it's destructive, so we never skip the guard.
        menu::Step::Confirm(Choice::Delete) => {
            app.palette = None;
            if let Some(path) = app.selected_path() {
                app.open_delete_confirm(path);
            }
            Step::Continue
        }
        menu::Step::Confirm(choice) => Step::Quit(Some(Outcome::Choice(choice))),
    })
}

fn handle_key(app: &mut App, k: crossterm::event::KeyEvent) -> Step {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);

    // Modals take routing precedence (confirm over palette over the picker):
    // while one is open every key routes to it.
    if let Some(step) = route_confirm_modal(app, k) {
        return step;
    }
    if let Some(step) = route_palette_modal(app, k) {
        return step;
    }

    match k.code {
        KeyCode::Esc => return Step::Quit(None),
        KeyCode::Char('c') if ctrl => return Step::Quit(None),
        KeyCode::Enter => {
            if let Some(p) = app.selected_path() {
                // Plain Enter → open the path directly (a directory match
                // reveals its folder downstream). Ctrl-Enter → reveal the
                // containing directory in Finder.
                let sel = if ctrl {
                    Selection::Reveal(p)
                } else {
                    Selection::Open(p)
                };
                return Step::Quit(Some(Outcome::Selected(sel)));
            }
        }

        // Ctrl-p opens the command palette overlay; up-navigation is
        // Ctrl-k / ↑ (down is Ctrl-n / Ctrl-j / ↓).
        KeyCode::Char('p') if ctrl => {
            // The one-shot picker has no brain panel, so "Message brain"
            // (the old launch-claude action) is always offered.
            app.open_palette(crate::state::PanelSide::DEFAULT, true);
        }

        // Ctrl-G opens the "Create PDF" confirmation modal when a markdown
        // file is highlighted; a no-op otherwise.
        KeyCode::Char('g') if ctrl => {
            if let Some(path) = app.selected_markdown_path() {
                app.open_confirm(path);
            }
        }
        // Ctrl-D opens the red "Delete" confirmation modal for the highlighted
        // entry (file or directory); a no-op when nothing is selected.
        KeyCode::Char('d') if ctrl => {
            if let Some(path) = app.selected_path() {
                app.open_delete_confirm(path);
            }
        }

        // Direct palette shortcuts that bypass the overlay (mirrors the
        // `tasks` convention; the same hints render dim in the palette).
        // Ctrl-m relies on the kitty protocol to stay distinct from Enter;
        // on a terminal without it, Ctrl-m degrades to plain Enter.
        KeyCode::Char('m') if ctrl => return Step::Quit(Some(Outcome::Choice(Choice::Msg))),
        KeyCode::Char('t') if ctrl => {
            return Step::Quit(Some(Outcome::Choice(Choice::OpenTasks)));
        }

        KeyCode::Up => app.move_up(),
        KeyCode::Char('k') if ctrl => app.move_up(),
        KeyCode::Down => app.move_down(),
        KeyCode::Char('n' | 'j') if ctrl => app.move_down(),

        KeyCode::PageUp => app.page_up(),
        KeyCode::PageDown => app.page_down(),
        KeyCode::Home => app.selected = 0,
        KeyCode::End => app.selected = app.matches.len().saturating_sub(1),

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

// ---------------------------------------------------------------------------
// Draw
// ---------------------------------------------------------------------------

fn draw(f: &mut Frame, app: &mut App) {
    draw_into(f, app, f.area());
}

/// Render the search panel into `area`.
///
/// Draws header / input / separator / list / footer, plus the palette
/// overlay. Used both full-screen by the one-shot picker and inside a
/// bordered sub-rect by the persistent two-panel TUI.
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

    // The command palette draws on top of the picker; the confirmation modal
    // draws last of all so it sits above even the palette.
    if let Some(palette) = &app.palette {
        menu::draw_modal(f, palette, area);
    }
    if let Some(c) = &app.confirm {
        confirm::draw_modal(f, c, area);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Bucket;
    use std::path::PathBuf;

    fn entry(bucket: Bucket, display: &str) -> Entry {
        Entry {
            path: PathBuf::from(display.replace('~', "/Users/x")),
            display: display.to_owned(),
            bucket,
        }
    }

    fn sample() -> Vec<Entry> {
        vec![
            entry(Bucket::Projects, "~/brain/projects/ann-afloat/plan.md"),
            entry(Bucket::Projects, "~/brain/projects/zebra/notes.md"),
            entry(Bucket::Areas, "~/brain/areas/health/log.md"),
            entry(Bucket::Resources, "~/brain/resources/rust/borrow.md"),
        ]
    }

    // --- bucket-relative directory paths (Open directory row) -----------

    #[test]
    fn bucket_relative_starts_at_the_category_segment() {
        assert_eq!(
            bucket_relative("~/brain/projects/foo/note.md", "projects").as_deref(),
            Some("projects/foo/note.md")
        );
        assert_eq!(
            bucket_relative("~/brain/resources/rust/borrow.md", "resources").as_deref(),
            Some("resources/rust/borrow.md")
        );
    }

    #[test]
    fn bucket_relative_matches_the_top_level_bucket_not_a_namesake_subdir() {
        // A later segment sharing the category name doesn't shadow the
        // top-level bucket (position finds the first match).
        assert_eq!(
            bucket_relative("~/brain/projects/projects/deep.md", "projects").as_deref(),
            Some("projects/projects/deep.md")
        );
    }

    #[test]
    fn bucket_relative_is_none_without_the_category() {
        assert_eq!(bucket_relative("~/somewhere/else/x.md", "projects"), None);
    }

    #[test]
    fn parent_reldisplay_drops_the_last_segment() {
        assert_eq!(parent_reldisplay("projects/foo/note.md"), "projects/foo");
        // A file directly under the bucket root → the bucket itself.
        assert_eq!(parent_reldisplay("projects/note.md"), "projects");
        // A lone segment (already the bucket root) is returned unchanged.
        assert_eq!(parent_reldisplay("projects"), "projects");
    }

    // --- HaystackBuf ----------------------------------------------------

    #[test]
    fn haystack_strips_slug_separators() {
        let h = HaystackBuf::new("ann-afloat_v.2");
        assert_eq!(h.normalized, "annafloatv2");
    }

    #[test]
    fn haystack_char_to_byte_map_round_trips() {
        let display = "a-b_c";
        let h = HaystackBuf::new(display);
        assert_eq!(h.normalized, "abc");
        // Each normalized char's recorded byte offset must point at the same
        // char in the original display string.
        for (norm_char_idx, ch) in h.normalized.chars().enumerate() {
            let byte = h.normalized_char_to_display_byte[norm_char_idx];
            assert_eq!(display[byte..].chars().next(), Some(ch));
        }
    }

    #[test]
    fn char_positions_map_to_display_bytes() {
        let h = HaystackBuf::new("ann-afloat");
        // "afloat" begins at normalized char index 3 ("ann" = 0,1,2).
        let positions = [3u32, 4, 5];
        let bytes = char_positions_to_byte_positions(&positions, &h.normalized_char_to_display_byte);
        // In the *display* string, 'a' of "afloat" sits after "ann-" → byte 4.
        assert!(bytes.contains(&4));
    }

    // --- refilter (matching + grouping + sort) --------------------------

    #[test]
    fn reload_entries_preserves_the_query_and_reflects_the_new_set() {
        // set_entries clears the query (a scope switch); reload_entries keeps
        // it (a refresh in place). After a reload the new entries drive the
        // still-active filter.
        let mut app = App::new(&sample(), "plan");
        assert_eq!(app.matches.len(), 1);
        let extra = vec![
            entry(Bucket::Projects, "~/brain/projects/ann-afloat/plan.md"),
            entry(Bucket::Areas, "~/brain/areas/health/plan.md"),
        ];
        app.reload_entries(&extra);
        assert_eq!(app.query, "plan");
        assert_eq!(app.matches.len(), 2);
    }

    #[test]
    fn drop_path_removes_the_entry_and_its_descendants() {
        // Trashing a directory drops everything beneath it; the query survives
        // and the filtered view shrinks accordingly.
        let mut app = App::new(&sample(), "");
        assert_eq!(app.matches.len(), 4);
        app.drop_path(&PathBuf::from("/Users/x/brain/projects/ann-afloat"));
        // ann-afloat/plan.md sits under the dropped directory, so it goes;
        // the other three entries remain.
        assert_eq!(app.matches.len(), 3);
    }

    #[test]
    fn empty_query_keeps_every_entry_grouped_by_bucket() {
        let entries = sample();
        let app = App::new(&entries, "");
        assert_eq!(app.matches.len(), 4);
        // Sorted P, P, A, R by bucket then entry order.
        let buckets: Vec<Bucket> = app.matches.iter().map(|m| m.bucket).collect();
        assert_eq!(
            buckets,
            vec![
                Bucket::Projects,
                Bucket::Projects,
                Bucket::Areas,
                Bucket::Resources
            ]
        );
    }

    #[test]
    fn slug_separators_do_not_block_a_substring_match() {
        let entries = sample();
        // "afloat" must find "ann-afloat" even though a dash splits the slug.
        let app = App::new(&entries, "afloat");
        assert_eq!(app.matches.len(), 1);
        assert_eq!(
            entries[app.matches[0].entry_idx].display,
            "~/brain/projects/ann-afloat/plan.md"
        );
    }

    #[test]
    fn query_with_no_hits_yields_no_matches() {
        let entries = sample();
        let app = App::new(&entries, "nonexistentxyz");
        assert!(app.matches.is_empty());
        assert!(app.display_rows.is_empty());
    }

    #[test]
    fn matched_entry_records_highlight_bytes() {
        let entries = sample();
        let app = App::new(&entries, "borrow");
        assert_eq!(app.matches.len(), 1);
        assert!(
            !app.matches[0].highlight_bytes.is_empty(),
            "a substring match must report highlight offsets"
        );
    }

    // --- display rows (section headers) ---------------------------------

    #[test]
    fn display_rows_insert_one_header_per_bucket() {
        let entries = sample();
        let app = App::new(&entries, "");
        // 3 buckets present (P, A, R) → 3 headers + 4 matches = 7 rows.
        assert_eq!(app.display_rows.len(), 7);
        let headers = app
            .display_rows
            .iter()
            .filter(|r| matches!(r, DisplayRow::Header(_, _)))
            .count();
        assert_eq!(headers, 3);
    }

    #[test]
    fn projects_header_counts_its_members() {
        let entries = sample();
        let app = App::new(&entries, "");
        match app.display_rows[0] {
            DisplayRow::Header(Bucket::Projects, count) => assert_eq!(count, 2),
            other => panic!("expected Projects header first, got {other:?}"),
        }
    }

    // --- navigation -----------------------------------------------------

    #[test]
    fn move_down_then_up_clamps_at_bounds() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        assert_eq!(app.selected, 0);
        app.move_up(); // already at top
        assert_eq!(app.selected, 0);
        app.move_down();
        app.move_down();
        assert_eq!(app.selected, 2);
        // Walk to the end and try to overshoot.
        app.move_down();
        app.move_down();
        assert_eq!(app.selected, 3);
    }

    #[test]
    fn page_down_and_up_saturate() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        app.page_down();
        assert_eq!(app.selected, app.matches.len() - 1);
        app.page_up();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn selected_path_tracks_the_cursor() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        app.move_down(); // index 1 → second projects entry
        assert_eq!(
            app.selected_path().unwrap(),
            entries[1].path
        );
    }

    #[test]
    fn selected_path_is_none_when_empty() {
        let entries: Vec<Entry> = Vec::new();
        let app = App::new(&entries, "");
        assert!(app.selected_path().is_none());
    }

    // --- Enter / Ctrl-Enter (open vs reveal) ----------------------------

    fn enter(ctrl: bool) -> crossterm::event::KeyEvent {
        let mods = if ctrl {
            KeyModifiers::CONTROL
        } else {
            KeyModifiers::NONE
        };
        crossterm::event::KeyEvent::new(KeyCode::Enter, mods)
    }

    #[test]
    fn plain_enter_opens_the_selection() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        match handle_key(&mut app, enter(false)) {
            Step::Quit(Some(Outcome::Selected(Selection::Open(p)))) => {
                assert_eq!(p, entries[0].path);
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_enter_reveals_the_directory() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        match handle_key(&mut app, enter(true)) {
            Step::Quit(Some(Outcome::Selected(Selection::Reveal(p)))) => {
                assert_eq!(p, entries[0].path);
            }
            other => panic!("expected Reveal, got {other:?}"),
        }
    }

    // --- command-palette overlay --------------------------------------

    fn ctrl(code: KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn plain(code: KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn ctrl_p_opens_the_palette_overlay_without_quitting() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        assert!(app.palette.is_none());
        match handle_key(&mut app, ctrl(KeyCode::Char('p'))) {
            Step::Continue => assert!(app.palette.is_some(), "Ctrl-p should open the overlay"),
            Step::Quit(out) => panic!("Ctrl-p should not quit, got {out:?}"),
        }
    }

    #[test]
    fn ctrl_k_navigates_up_and_does_not_open_the_palette() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        app.move_down();
        let before = app.selected;
        handle_key(&mut app, ctrl(KeyCode::Char('k')));
        assert!(app.palette.is_none(), "Ctrl-k must not open the palette");
        assert_eq!(app.selected, before - 1, "Ctrl-k should move selection up");
    }

    #[test]
    fn esc_in_the_palette_returns_to_the_picker() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        handle_key(&mut app, ctrl(KeyCode::Char('p')));
        assert!(app.palette.is_some());
        // Esc closes the overlay and keeps the picker alive (does NOT quit).
        match handle_key(&mut app, plain(KeyCode::Esc)) {
            Step::Continue => assert!(app.palette.is_none(), "Esc should close the overlay"),
            Step::Quit(out) => panic!("Esc in the palette should not quit, got {out:?}"),
        }
    }

    #[test]
    fn typing_in_the_palette_does_not_touch_the_picker_query() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        handle_key(&mut app, ctrl(KeyCode::Char('p')));
        handle_key(&mut app, plain(KeyCode::Char('g')));
        // The keystroke filtered the palette, not the picker's own query.
        assert_eq!(app.query, "");
        assert!(app.palette.is_some());
    }

    #[test]
    fn enter_in_the_palette_confirms_a_choice() {
        // Any highlighted entry now leads with the "Open directory" row, so to
        // exercise the plain-`Outcome::Choice` path we filter down to a static
        // row ("Message brain") first, then confirm it with Enter.
        let entries = vec![entry(Bucket::Resources, "~/brain/resources/scan.pdf")];
        let mut app = App::new(&entries, "");
        handle_key(&mut app, ctrl(KeyCode::Char('p')));
        for c in "message".chars() {
            handle_key(&mut app, plain(KeyCode::Char(c)));
        }
        match handle_key(&mut app, plain(KeyCode::Enter)) {
            Step::Quit(Some(Outcome::Choice(Choice::Msg))) => {}
            other => panic!("expected Msg choice, got {other:?}"),
        }
    }

    // --- direct palette shortcuts (Ctrl-m / Ctrl-t / Ctrl-b) ------------

    #[test]
    fn ctrl_m_fires_message_brain_directly() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        match handle_key(&mut app, ctrl(KeyCode::Char('m'))) {
            Step::Quit(Some(Outcome::Choice(Choice::Msg))) => {}
            other => panic!("expected Msg choice, got {other:?}"),
        }
        assert!(app.palette.is_none(), "the shortcut should bypass the palette");
    }

    #[test]
    fn ctrl_t_fires_open_tasks_directly() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        match handle_key(&mut app, ctrl(KeyCode::Char('t'))) {
            Step::Quit(Some(Outcome::Choice(Choice::OpenTasks))) => {}
            other => panic!("expected OpenTasks choice, got {other:?}"),
        }
    }

    // --- Create PDF (Ctrl-G + confirmation modal) ----------------------

    #[test]
    fn selected_markdown_path_tracks_only_markdown_entries() {
        let entries = vec![
            entry(Bucket::Projects, "~/brain/projects/foo/plan.md"),
            entry(Bucket::Resources, "~/brain/resources/scan.pdf"),
        ];
        let mut app = App::new(&entries, "");
        // First entry is markdown.
        assert!(app.selected_markdown_path().is_some());
        assert_eq!(app.selected_markdown_filename().as_deref(), Some("plan.md"));
        // Move to the .pdf entry → not markdown.
        app.move_down();
        assert!(app.selected_markdown_path().is_none());
        assert!(app.selected_markdown_filename().is_none());
    }

    #[test]
    fn ctrl_g_on_markdown_opens_the_confirmation_modal() {
        let entries = sample(); // first entry is a .md file
        let mut app = App::new(&entries, "");
        match handle_key(&mut app, ctrl(KeyCode::Char('g'))) {
            Step::Continue => assert!(app.confirm.is_some(), "Ctrl-g should open the modal"),
            Step::Quit(out) => panic!("Ctrl-g should not quit, got {out:?}"),
        }
    }

    #[test]
    fn ctrl_g_on_non_markdown_is_a_noop() {
        let entries = vec![entry(Bucket::Resources, "~/brain/resources/scan.pdf")];
        let mut app = App::new(&entries, "");
        match handle_key(&mut app, ctrl(KeyCode::Char('g'))) {
            Step::Continue => assert!(app.confirm.is_none(), "no modal for a non-markdown file"),
            Step::Quit(out) => panic!("Ctrl-g should not quit, got {out:?}"),
        }
    }

    #[test]
    fn confirming_the_modal_quits_with_create_pdf() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        handle_key(&mut app, ctrl(KeyCode::Char('g')));
        // Default highlight is Yes, so Enter accepts.
        match handle_key(&mut app, plain(KeyCode::Enter)) {
            Step::Quit(Some(Outcome::CreatePdf(p))) => assert_eq!(p, entries[0].path),
            other => panic!("expected CreatePdf, got {other:?}"),
        }
    }

    #[test]
    fn declining_the_modal_returns_to_the_picker() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        handle_key(&mut app, ctrl(KeyCode::Char('g')));
        match handle_key(&mut app, plain(KeyCode::Esc)) {
            Step::Continue => assert!(app.confirm.is_none(), "Esc should close the modal"),
            Step::Quit(out) => panic!("Esc in the modal should not quit, got {out:?}"),
        }
    }

    #[test]
    fn palette_create_pdf_row_quits_with_create_pdf() {
        let entries = sample();
        let mut app = App::new(&entries, "");
        // Open the palette on a markdown file → "Create PDF" leads the list.
        handle_key(&mut app, ctrl(KeyCode::Char('p')));
        match handle_key(&mut app, plain(KeyCode::Enter)) {
            Step::Quit(Some(Outcome::CreatePdf(p))) => assert_eq!(p, entries[0].path),
            other => panic!("expected CreatePdf from the palette, got {other:?}"),
        }
    }
}
