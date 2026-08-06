//! Building and (re)filtering the match set: the constructors that seed the
//! searchable entries, the `refilter` pass (nucleo substring matching + bucket
//! sort), and the section-header grouping that turns matches into display rows.

use std::collections::BTreeSet;

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

use crate::entry::Entry;

use super::haystack::{HaystackBuf, char_positions_to_byte_positions};
use super::{App, DisplayRow, Match};

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

    pub(super) fn refilter(&mut self) {
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
}
