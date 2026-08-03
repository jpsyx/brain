//! Pure UUID-aware three-way merge for task and habit CSVs.

mod merge;
mod reconcile;
mod relationships;
mod table;

pub use merge::{Report, merge};
pub use relationships::{project_task_lists, rewrite_project_metadata};
pub use table::{Table, parse, serialize, validate_for_merge};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Table, merge, parse, serialize};

    const H: &[&str] = &[
        "task_id",
        "status",
        "notes",
        "due_date",
        "priority",
        "completed_date",
        "last_touched",
    ];

    fn tbl(header: &[&str], rows: &[&[&str]]) -> Table {
        let header = header.iter().map(|value| (*value).to_owned()).collect();
        let mut map = BTreeMap::new();
        for row in rows {
            let cells = row
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>();
            map.insert(cells[0].clone(), cells);
        }
        Table { header, rows: map }
    }

    fn cell(table: &Table, id: &str, column: &str) -> String {
        let index = table
            .header
            .iter()
            .position(|candidate| candidate == column)
            .unwrap();
        table.rows[id][index].clone()
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
        let table = parse("task_id,status,notes\n1,open\n");
        assert_eq!(
            table.rows["1"],
            vec!["1".to_owned(), "open".to_owned(), String::new()]
        );
    }

    #[test]
    fn empty_text_is_empty_table() {
        let table = parse("");
        assert!(table.header.is_empty());
        assert!(table.rows.is_empty());
        assert_eq!(serialize(&table), "");
    }

    #[test]
    fn add_on_one_side_is_kept() {
        let base = tbl(H, &[]);
        let ours = tbl(H, &[&["1", "open", "n", "", "", "", "t0"]]);
        let theirs = tbl(H, &[]);
        let (merged, report) = merge(&base, &ours, &theirs);
        assert_eq!(serialize(&merged), serialize(&ours));
        assert_eq!(report.added, 1);
    }

    #[test]
    fn add_same_id_on_both_field_merges() {
        let base = tbl(H, &[]);
        let row: &[&str] = &["1", "open", "n", "", "", "", "t0"];
        let ours = tbl(H, &[row]);
        let theirs = tbl(H, &[row]);
        let (merged, report) = merge(&base, &ours, &theirs);
        assert_eq!(merged.rows["1"], ours.rows["1"]);
        assert_eq!(report.added, 1);
    }

    #[test]
    fn complete_on_one_side_edit_on_the_other() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "t0"]]);
        let ours = tbl(H, &[&["1", "done", "orig", "", "", "2026-07-25", "t1"]]);
        let theirs = tbl(H, &[&["1", "open", "EDITED", "", "", "", "t2"]]);
        let (merged, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&merged, "1", "status"), "done");
        assert_eq!(cell(&merged, "1", "completed_date"), "2026-07-25");
        assert_eq!(cell(&merged, "1", "notes"), "EDITED");
    }

    #[test]
    fn delete_vs_unchanged_deletes() {
        let base = tbl(H, &[&["1", "open", "n", "", "", "", "t0"]]);
        let ours = tbl(H, &[]);
        let theirs = base.clone();
        let (merged, report) = merge(&base, &ours, &theirs);
        assert!(!merged.rows.contains_key("1"));
        assert_eq!(report.deleted, 1);
    }

    #[test]
    fn delete_vs_edited_keeps_the_edit_and_notes() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "t0"]]);
        let ours = tbl(H, &[]);
        let theirs = tbl(H, &[&["1", "open", "EDITED", "", "", "", "t2"]]);
        let (merged, report) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&merged, "1", "notes"), "EDITED");
        assert!(
            report
                .soft_conflicts
                .iter()
                .any(|note| note.contains("deleted on one side"))
        );
    }

    #[test]
    fn field_union_keeps_both_disjoint_changes() {
        let base = tbl(H, &[&["1", "open", "n", "d0", "p0", "", "t0"]]);
        let ours = tbl(H, &[&["1", "open", "n", "D1", "p0", "", "t1"]]);
        let theirs = tbl(H, &[&["1", "open", "n", "d0", "P1", "", "t2"]]);
        let (merged, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&merged, "1", "due_date"), "D1");
        assert_eq!(cell(&merged, "1", "priority"), "P1");
    }

    #[test]
    fn same_field_last_write_wins_newer_last_touched() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "2026-01-01"]]);
        let ours = tbl(H, &[&["1", "open", "OURS", "", "", "", "2026-02-01"]]);
        let theirs = tbl(H, &[&["1", "open", "THEIRS", "", "", "", "2026-06-01"]]);
        let (merged, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&merged, "1", "notes"), "THEIRS");
    }

    #[test]
    fn completion_wins_even_when_other_last_touched_is_newer() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "2026-01-01"]]);
        let ours = tbl(H, &[&["1", "done", "orig", "", "", "C", "2026-01-02"]]);
        let theirs = tbl(H, &[&["1", "open", "EDIT", "", "", "", "2026-12-01"]]);
        let (merged, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&merged, "1", "status"), "done");
        assert_eq!(cell(&merged, "1", "completed_date"), "C");
    }

    #[test]
    fn both_set_same_value_no_conflict() {
        let base = tbl(H, &[&["1", "open", "orig", "", "", "", "t0"]]);
        let ours = tbl(H, &[&["1", "open", "SAME", "", "", "", "t1"]]);
        let theirs = tbl(H, &[&["1", "open", "SAME", "", "", "", "t2"]]);
        let (merged, report) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&merged, "1", "notes"), "SAME");
        assert!(report.soft_conflicts.is_empty());
    }

    #[test]
    fn merge_aligns_reordered_headers_by_column_name() {
        let base = tbl(
            &["task_id", "status", "notes", "last_touched"],
            &[&["1", "open", "original", "2026-01-01"]],
        );
        let ours = tbl(
            &["task_id", "notes", "status", "last_touched"],
            &[&["1", "local note", "open", "2026-02-01"]],
        );
        let theirs = tbl(
            &["task_id", "status", "notes", "last_touched"],
            &[&["1", "done", "original", "2026-03-01"]],
        );
        let (merged, _) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&merged, "1", "task_id"), "1");
        assert_eq!(cell(&merged, "1", "status"), "done");
        assert_eq!(cell(&merged, "1", "notes"), "local note");
        assert_eq!(cell(&merged, "1", "last_touched"), "2026-03-01");
    }

    #[test]
    fn empty_base_unions_both_sides() {
        let base = tbl(H, &[]);
        let ours = tbl(H, &[&["1", "open", "x", "", "", "", "t0"]]);
        let theirs = tbl(H, &[&["2", "open", "y", "", "", "", "t0"]]);
        let (merged, _) = merge(&base, &ours, &theirs);
        assert!(merged.rows.contains_key("1"));
        assert!(merged.rows.contains_key("2"));
    }

    const HAB: &[&str] = &["task_id", "status", "notes"];

    #[test]
    fn missing_last_touched_uses_lexicographic_fallback_with_note() {
        let base = tbl(HAB, &[&["1", "open", "orig"]]);
        let ours = tbl(HAB, &[&["1", "open", "apple"]]);
        let theirs = tbl(HAB, &[&["1", "open", "zebra"]]);
        let (merged, report) = merge(&base, &ours, &theirs);
        assert_eq!(cell(&merged, "1", "notes"), "zebra");
        assert!(
            report
                .soft_conflicts
                .iter()
                .any(|note| note.contains("notes"))
        );
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
        let merged = merge(&base, &ours, &theirs).0;
        let again = merge(&merged, &merged, &merged).0;
        assert_eq!(again, merged);
    }

    #[test]
    fn convergence_swapping_ours_and_theirs_is_byte_identical() {
        let (base, ours, theirs) = rich_case();
        let first = serialize(&merge(&base, &ours, &theirs).0);
        let mirrored = serialize(&merge(&base, &theirs, &ours).0);
        assert_eq!(first, mirrored);
    }
}
