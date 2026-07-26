//! Pure id-keyed 3-way merge for the tasks/habits CSVs.
//!
//! The merge is deterministic, convergent, and idempotent so two machines that
//! start from the same last-synced snapshot converge to a byte-identical result
//! and stop conflicting. Keyed by `task_id` (the first column). No IO lives
//! here; the transport layer reads/writes files and calls [`merge`].

use std::collections::{BTreeMap, BTreeSet};

/// A parsed CSV table keyed by `task_id`, preserving column order via `header`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Column order (`task_id` first).
    pub header: Vec<String>,
    /// `task_id` -> row cells (aligned to `header`).
    pub rows: BTreeMap<String, Vec<String>>,
}

/// Soft notes worth surfacing (journal / status), never fatal.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Rows that were newly added on one or both sides.
    pub added: usize,
    /// Rows removed because a delete won.
    pub deleted: usize,
    /// Rows present on both sides that were field-merged.
    pub merged: usize,
    /// Human-readable notes, e.g. a delete-vs-edit that kept the edit.
    pub soft_conflicts: Vec<String>,
}

/// Parse CSV text into a [`Table`] (header + `task_id`-keyed rows).
///
/// A row shorter or longer than the header is padded or truncated to the header
/// length. Empty text yields an empty table with an empty header. Uses the `csv`
/// crate in flexible mode.
#[must_use]
pub fn parse(text: &str) -> Table {
    if text.is_empty() {
        return Table { header: Vec::new(), rows: BTreeMap::new() };
    }
    let mut rdr =
        csv::ReaderBuilder::new().has_headers(true).flexible(true).from_reader(text.as_bytes());
    let header: Vec<String> =
        rdr.headers().map(|h| h.iter().map(ToOwned::to_owned).collect()).unwrap_or_default();
    let width = header.len();
    let mut rows = BTreeMap::new();
    for rec in rdr.records().flatten() {
        let mut cells: Vec<String> = rec.iter().map(ToOwned::to_owned).collect();
        if cells.is_empty() {
            continue;
        }
        let key = cells[0].clone();
        if width > 0 {
            cells.resize(width, String::new());
        }
        rows.insert(key, cells);
    }
    Table { header, rows }
}

/// Serialize a [`Table`] back to CSV text.
///
/// Writes the header, then rows sorted by `task_id` (the `BTreeMap` already
/// sorts), producing byte-identical output for equal tables.
#[must_use]
pub fn serialize(t: &Table) -> String {
    let mut wtr = csv::WriterBuilder::new().from_writer(Vec::new());
    if !t.header.is_empty() {
        let _ = wtr.write_record(&t.header);
    }
    for row in t.rows.values() {
        let _ = wtr.write_record(row);
    }
    let bytes = wtr.into_inner().unwrap_or_default();
    String::from_utf8(bytes).unwrap_or_default()
}

/// Semantic column indices, resolved once from the chosen header.
struct Cols {
    status: Option<usize>,
    completed: Option<usize>,
    last_touched: Option<usize>,
}

impl Cols {
    fn from_header(header: &[String]) -> Self {
        let find = |name: &str| header.iter().position(|c| c == name);
        Self { status: find("status"), completed: find("completed_date"), last_touched: find("last_touched") }
    }
}

/// Pick the surviving header: the non-empty table with the most columns,
/// breaking ties in favour of `ours`, then `theirs`, then `base`.
fn choose_header(base: &Table, ours: &Table, theirs: &Table) -> Vec<String> {
    let mut best = &ours.header;
    if theirs.header.len() > best.len() {
        best = &theirs.header;
    }
    if base.header.len() > best.len() {
        best = &base.header;
    }
    best.clone()
}

/// Pad or truncate `row` to `width` by index.
fn norm(row: &[String], width: usize) -> Vec<String> {
    let mut v = row.to_vec();
    v.resize(width, String::new());
    v
}

/// Resolve a single genuine conflict (both sides changed a cell to different,
/// non-equal values, and it is not a completion cell). Deterministic and
/// side-independent so the merge converges.
fn resolve_conflict(
    o: &[String],
    t: &[String],
    cols: &Cols,
    header: &[String],
    i: usize,
    id: &str,
    notes: &mut Vec<String>,
) -> String {
    let (oi, ti) = (&o[i], &t[i]);
    cols.last_touched.map_or_else(
        || {
            // No usable `last_touched`: the lexicographically-greater value
            // wins deterministically, and we flag the collision.
            let col = header.get(i).map_or("", String::as_str);
            notes.push(format!("task_id {id}: conflicting {col} values; kept the greater"));
            if oi >= ti { oi.clone() } else { ti.clone() }
        },
        |lt| {
            // Greater `last_touched` wins; ties break on the greater cell value
            // so the result is independent of side (converges).
            if (o[lt].as_str(), oi.as_str()) >= (t[lt].as_str(), ti.as_str()) {
                oi.clone()
            } else {
                ti.clone()
            }
        },
    )
}

/// Field-level merge of two changed rows against an optional base. Completion is
/// resolved at the row level first (a `done` side dictates `status` +
/// `completed_date`), then every other column is merged cell-by-cell.
fn field_merge(
    base: Option<&[String]>,
    o: &[String],
    t: &[String],
    cols: &Cols,
    header: &[String],
    id: &str,
) -> (Vec<String>, Vec<String>) {
    let width = header.len();
    let mut out = vec![String::new(); width];
    let mut resolved = vec![false; width];
    let mut notes = Vec::new();

    let o_done = cols.status.is_some_and(|i| o[i] == "done");
    let t_done = cols.status.is_some_and(|i| t[i] == "done");
    if o_done != t_done {
        let done = if o_done { o } else { t };
        for idx in [cols.status, cols.completed].into_iter().flatten() {
            out[idx].clone_from(&done[idx]);
            resolved[idx] = true;
        }
    }

    for i in 0..width {
        if resolved[i] {
            continue;
        }
        let (oi, ti) = (&o[i], &t[i]);
        out[i] = match base {
            Some(b) if oi == &b[i] => ti.clone(),
            Some(b) if ti == &b[i] => oi.clone(),
            _ if oi == ti => oi.clone(),
            _ => resolve_conflict(o, t, cols, header, i, id, &mut notes),
        };
    }
    (out, notes)
}

/// 3-way merge keyed by `task_id`.
///
/// `base` is the last-synced snapshot, `ours` the local table, `theirs` the
/// remote one. Returns the merged table plus a [`Report`]. The header is taken
/// from whichever non-empty table has the most columns (preferring `ours`, then
/// `theirs`, then `base`) so a schema superset survives.
#[must_use]
pub fn merge(base: &Table, ours: &Table, theirs: &Table) -> (Table, Report) {
    let header = choose_header(base, ours, theirs);
    let width = header.len();
    let cols = Cols::from_header(&header);
    let mut rows = BTreeMap::new();
    let mut report = Report::default();

    let ids: BTreeSet<&str> = base
        .rows
        .keys()
        .chain(ours.rows.keys())
        .chain(theirs.rows.keys())
        .map(String::as_str)
        .collect();

    for id in ids {
        let b = base.rows.get(id).map(|r| norm(r, width));
        let o = ours.rows.get(id).map(|r| norm(r, width));
        let t = theirs.rows.get(id).map(|r| norm(r, width));

        match (b, o, t) {
            (None, Some(o), None) | (None, None, Some(o)) => {
                rows.insert(id.to_owned(), o);
                report.added += 1;
            }
            (None, Some(o), Some(t)) => {
                let (row, notes) = field_merge(None, &o, &t, &cols, &header, id);
                rows.insert(id.to_owned(), row);
                report.added += 1;
                report.soft_conflicts.extend(notes);
            }
            (Some(_), None, None) => report.deleted += 1,
            (Some(b), Some(side), None) | (Some(b), None, Some(side)) => {
                if side == b {
                    report.deleted += 1;
                } else {
                    rows.insert(id.to_owned(), side);
                    report.soft_conflicts.push(format!(
                        "task_id {id}: deleted on one side but edited on the other; kept the edit"
                    ));
                }
            }
            (Some(b), Some(o), Some(t)) => match (o != b, t != b) {
                (false, false) => {
                    rows.insert(id.to_owned(), b);
                }
                (true, false) => {
                    rows.insert(id.to_owned(), o);
                }
                (false, true) => {
                    rows.insert(id.to_owned(), t);
                }
                (true, true) => {
                    let (row, notes) = field_merge(Some(&b), &o, &t, &cols, &header, id);
                    rows.insert(id.to_owned(), row);
                    report.merged += 1;
                    report.soft_conflicts.extend(notes);
                }
            },
            (None, None, None) => {}
        }
    }

    (Table { header, rows }, report)
}

#[cfg(test)]
mod tests {
    use super::{merge, parse, serialize, Table};
    use std::collections::BTreeMap;

    /// Task-like schema used across the merge tests.
    const H: &[&str] =
        &["task_id", "status", "notes", "due_date", "priority", "completed_date", "last_touched"];

    fn tbl(header: &[&str], rows: &[&[&str]]) -> Table {
        let header = header.iter().map(|s| (*s).to_owned()).collect();
        let mut map = BTreeMap::new();
        for r in rows {
            let cells: Vec<String> = r.iter().map(|s| (*s).to_owned()).collect();
            map.insert(cells[0].clone(), cells);
        }
        Table { header, rows: map }
    }

    fn cell(t: &Table, id: &str, col: &str) -> String {
        let idx = t.header.iter().position(|c| c == col).unwrap();
        t.rows[id][idx].clone()
    }

    #[test]
    fn parse_serialize_round_trip() {
        let text = "task_id,status,notes\n1,open,hello\n2,done,world\n";
        assert_eq!(serialize(&parse(text)), text);
    }

    #[test]
    fn rows_come_out_task_id_sorted_regardless_of_input_order() {
        let text = "task_id,status\n3,a\n1,b\n2,c\n";
        assert_eq!(serialize(&parse(text)), "task_id,status\n1,b\n2,c\n3,a\n");
    }

    #[test]
    fn short_rows_are_padded_to_header_width() {
        let t = parse("task_id,status,notes\n1,open\n");
        assert_eq!(t.rows["1"], vec!["1".to_owned(), "open".to_owned(), String::new()]);
    }

    #[test]
    fn empty_text_is_empty_table() {
        let t = parse("");
        assert!(t.header.is_empty());
        assert!(t.rows.is_empty());
        assert_eq!(serialize(&t), "");
    }

    #[test]
    fn add_on_one_side_is_kept() {
        let base = tbl(H, &[]);
        let ours = tbl(H, &[&["1", "open", "n", "", "", "", "t0"]]);
        let theirs = tbl(H, &[]);
        let (m, rep) = merge(&base, &ours, &theirs);
        assert_eq!(serialize(&m), serialize(&ours));
        assert_eq!(rep.added, 1);
    }

    #[test]
    fn add_same_id_on_both_field_merges() {
        let base = tbl(H, &[]);
        let row: &[&str] = &["1", "open", "n", "", "", "", "t0"];
        let ours = tbl(H, &[row]);
        let theirs = tbl(H, &[row]);
        let (m, rep) = merge(&base, &ours, &theirs);
        assert_eq!(m.rows["1"], ours.rows["1"]);
        assert_eq!(rep.added, 1);
    }

    #[test]
    fn complete_on_one_side_edit_on_the_other() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "t0"]]);
        let ours = tbl(H, &[&["1", "done", "orig", "", "", "2026-07-25", "t1"]]);
        let theirs = tbl(H, &[&["1", "open", "EDITED", "", "", "", "t2"]]);
        let (m, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&m, "1", "status"), "done");
        assert_eq!(cell(&m, "1", "completed_date"), "2026-07-25");
        assert_eq!(cell(&m, "1", "notes"), "EDITED");
    }

    #[test]
    fn delete_vs_unchanged_deletes() {
        let base = tbl(H, &[&["1", "open", "n", "", "", "", "t0"]]);
        let ours = tbl(H, &[]);
        let theirs = base.clone();
        let (m, rep) = merge(&base, &ours, &theirs);
        assert!(!m.rows.contains_key("1"));
        assert_eq!(rep.deleted, 1);
    }

    #[test]
    fn delete_vs_edited_keeps_the_edit_and_notes() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "t0"]]);
        let ours = tbl(H, &[]);
        let theirs = tbl(H, &[&["1", "open", "EDITED", "", "", "", "t2"]]);
        let (m, rep) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&m, "1", "notes"), "EDITED");
        assert!(rep.soft_conflicts.iter().any(|s| s.contains("deleted on one side")));
    }

    #[test]
    fn field_union_keeps_both_disjoint_changes() {
        let base = tbl(H, &[&["1", "open", "n", "d0", "p0", "", "t0"]]);
        let ours = tbl(H, &[&["1", "open", "n", "D1", "p0", "", "t1"]]);
        let theirs = tbl(H, &[&["1", "open", "n", "d0", "P1", "", "t2"]]);
        let (m, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&m, "1", "due_date"), "D1");
        assert_eq!(cell(&m, "1", "priority"), "P1");
    }

    #[test]
    fn same_field_last_write_wins_newer_last_touched() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "2026-01-01"]]);
        let ours = tbl(H, &[&["1", "open", "OURS", "", "", "", "2026-02-01"]]);
        let theirs = tbl(H, &[&["1", "open", "THEIRS", "", "", "", "2026-06-01"]]);
        let (m, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&m, "1", "notes"), "THEIRS");
    }

    #[test]
    fn completion_wins_even_when_other_last_touched_is_newer() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "2026-01-01"]]);
        let ours = tbl(H, &[&["1", "done", "orig", "", "", "C", "2026-01-02"]]);
        let theirs = tbl(H, &[&["1", "open", "EDIT", "", "", "", "2026-12-01"]]);
        let (m, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&m, "1", "status"), "done");
        assert_eq!(cell(&m, "1", "completed_date"), "C");
    }

    #[test]
    fn both_set_same_value_no_conflict() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "t0"]]);
        let ours = tbl(H, &[&["1", "open", "SAME", "", "", "", "t1"]]);
        let theirs = tbl(H, &[&["1", "open", "SAME", "", "", "", "t2"]]);
        let (m, rep) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&m, "1", "notes"), "SAME");
        assert!(rep.soft_conflicts.is_empty());
    }

    #[test]
    fn empty_base_unions_both_sides() {
        let base = tbl(H, &[]);
        let ours = tbl(H, &[&["1", "open", "x", "", "", "", "t0"]]);
        let theirs = tbl(H, &[&["2", "open", "y", "", "", "", "t0"]]);
        let (m, _) = merge(&base, &ours, &theirs);
        assert!(m.rows.contains_key("1"));
        assert!(m.rows.contains_key("2"));
    }

    // Legacy/no-column tables fall back to lexicographically-greater with a
    // soft note.
    const HAB: &[&str] = &["task_id", "status", "notes"];

    #[test]
    fn missing_last_touched_uses_lexicographic_fallback_with_note() {
        let base = tbl(HAB, &[&["1", "open", "orig"]]);
        let ours = tbl(HAB, &[&["1", "open", "apple"]]);
        let theirs = tbl(HAB, &[&["1", "open", "zebra"]]);
        let (m, rep) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&m, "1", "notes"), "zebra");
        assert!(rep.soft_conflicts.iter().any(|s| s.contains("notes")));
    }

    fn rich_case() -> (Table, Table, Table) {
        let base = tbl(
            H,
            &[
                &["1", "open", "orig", "", "", "", "2026-01-01"],
                &["2", "open", "foo", "", "", "", "2026-01-01"],
            ],
        );
        let ours = tbl(
            H,
            &[
                &["1", "done", "orig", "", "", "C1", "2026-02-01"],
                &["2", "open", "BAR_OURS", "", "", "", "2026-03-01"],
            ],
        );
        let theirs = tbl(
            H,
            &[
                &["1", "open", "orig", "", "", "", "2026-05-01"],
                &["2", "open", "BAR_THEIRS", "", "", "", "2026-04-01"],
            ],
        );
        (base, ours, theirs)
    }

    #[test]
    fn idempotency_merging_a_merged_table_with_itself_is_a_no_op() {
        let (base, ours, theirs) = rich_case();
        let m = merge(&base, &ours, &theirs).0;
        let again = merge(&m, &m, &m).0;
        assert_eq!(again, m);
    }

    #[test]
    fn convergence_swapping_ours_and_theirs_is_byte_identical() {
        let (base, ours, theirs) = rich_case();
        let a = serialize(&merge(&base, &ours, &theirs).0);
        let b = serialize(&merge(&base, &theirs, &ours).0);
        assert_eq!(a, b);
    }
}
