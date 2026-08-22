//! Ratatui-based fuzzy picker over collected `~/brain` entries.
//!
//! This module owns the picker's state (`App`), its matching/grouping logic,
//! and its rendering (`draw_into`). The interactive event loop lives in the
//! persistent shell (`tui/`), which embeds this `App` as the brain-search main
//! view and drives its keys directly — there is no standalone picker process.
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
//!
//! Layout (the `App` type lives here so every submodule can reach its private
//! fields; the impls are split by concern):
//!   - `haystack`  — per-entry match preprocessing + highlight mapping
//!   - `filter`    — constructors, `refilter`, section grouping
//!   - `nav`       — query edits + cursor movement + scroll
//!   - `selection`: the highlighted entry's path/filename/dir accessors and
//!     shell-owned palette construction data
//!   - `view`      — `draw_into` and its helpers

mod filter;
mod haystack;
mod nav;
mod selection;
mod view;

use std::collections::BTreeSet;

use nucleo_matcher::Matcher;

use crate::entry::{Bucket, Entry};

use haystack::HaystackBuf;

pub use view::draw_into;

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
    /// Owned so the persistent TUI can rescope the search to a different
    /// bucket set in place with `set_entries`.
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
}
