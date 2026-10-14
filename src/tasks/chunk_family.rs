//! Shared gap-aware resolution of numbered task sessions.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chunk {
    pub id: String,
    pub base: String,
    pub index: u32,
    pub total: u32,
    pub project: String,
    pub done: bool,
}

use std::collections::BTreeSet;

use super::complete::{Row, field, parse_chunk_name};

/// Parse one CSV row using the existing numbered-session name grammar.
pub(crate) fn parse(row: &Row) -> Option<Chunk> {
    let (base, index, total) = parse_chunk_name(&field(row, "task_name"))?;
    Some(Chunk {
        id: field(row, "task_id").trim().to_owned(),
        base,
        index,
        total,
        project: field(row, "project").trim().to_owned(),
        done: field(row, "status").trim() == "done",
    })
}

fn same_family(left: &Chunk, right: &Chunk) -> bool {
    left.base == right.base && left.total == right.total && left.project == right.project
}

fn validate_positions(anchor: &Chunk, rows: &[Chunk]) -> Result<(), String> {
    let mut positions = BTreeSet::new();
    for row in rows.iter().filter(|row| same_family(anchor, row)) {
        if !positions.insert(row.index) {
            return Err("ambiguous_chunk_family".into());
        }
    }
    Ok(())
}

/// Validate the full anchor family before selecting from a surviving subset.
/// Same-title dependency edges crossing a family key are conflicting evidence;
/// unrelated-title dependencies do not affect family identity or eligibility.
pub(crate) fn validate(anchor: &Chunk, rows: &[Row]) -> Result<Vec<Chunk>, String> {
    let chunks: Vec<_> = rows.iter().filter_map(parse).collect();
    validate_positions(anchor, &chunks)?;
    for row in rows {
        let Some(source) = parse(row) else { continue };
        if source.base != anchor.base {
            continue;
        }
        for dependency in field(row, "blocked_by").split('|').map(str::trim) {
            for target in chunks.iter().filter(|chunk| chunk.id == dependency) {
                if target.base == anchor.base
                    && (same_family(anchor, &source) || same_family(anchor, target))
                    && !same_family(&source, target)
                {
                    return Err("ambiguous_chunk_family".into());
                }
            }
        }
    }
    Ok(chunks)
}

/// Select the earliest unfinished surviving session after the anchor.
/// Visibility and dependency eligibility are deliberately left to callers.
pub(crate) fn next<'a>(anchor: &Chunk, rows: &'a [Chunk]) -> Result<Option<&'a Chunk>, String> {
    validate_positions(anchor, rows)?;
    Ok(rows
        .iter()
        .filter(|row| same_family(anchor, row) && row.index > anchor.index && !row.done)
        .min_by_key(|row| row.index))
}

pub(crate) fn successor<'a>(anchor_row: &Row, rows: &'a [Row]) -> Result<Option<&'a Row>, String> {
    let Some(anchor) = parse(anchor_row) else {
        return Ok(None);
    };
    let chunks = validate(&anchor, rows)?;
    let Some(target) = next(&anchor, &chunks)? else {
        return Ok(None);
    };
    Ok(rows.iter().find(|row| parse(row).as_ref() == Some(target)))
}

#[cfg(test)]
mod tests {
    use super::{Chunk, next, parse, successor, validate};
    use crate::tasks::complete::Row;

    fn chunk(id: &str, index: u32) -> Chunk {
        Chunk {
            id: id.into(),
            base: "Read specification".into(),
            index,
            total: 4,
            project: "alpha".into(),
            done: false,
        }
    }

    #[test]
    fn successor_skips_a_removed_session() {
        let rows = [chunk("T1", 1), chunk("T3", 3), chunk("T4", 4)];
        assert_eq!(next(&rows[0], &rows).unwrap().unwrap().id, "T3");
    }

    #[test]
    fn successors_respect_gaps_done_rows_and_family_keys() {
        let mut rows = vec![chunk("T1", 1), chunk("T3", 3), chunk("T4", 4)];
        for (index, expected) in [(0, Some("T3")), (1, Some("T4")), (2, None)] {
            assert_eq!(
                next(&rows[index], &rows).unwrap().map(|c| c.id.as_str()),
                expected
            );
        }
        rows[1].done = true;
        assert_eq!(next(&rows[0], &rows).unwrap().unwrap().id, "T4");
        let mut other_project = chunk("T2", 2);
        other_project.project = "beta".into();
        let mut other_total = chunk("T9", 2);
        other_total.total = 9;
        rows.extend([other_project, other_total]);
        assert_eq!(next(&rows[0], &rows).unwrap().unwrap().id, "T4");
        assert_eq!(next(&chunk("T0", 0), &rows[1..]).unwrap().unwrap().id, "T4");
    }

    #[test]
    fn duplicate_positions_fail_even_when_done_or_before_anchor() {
        for duplicate_index in [1, 3, 4] {
            let mut duplicate = chunk("T9", duplicate_index);
            duplicate.done = true;
            let rows = [chunk("T1", 1), chunk("T3", 3), chunk("T4", 4), duplicate];
            assert_eq!(next(&rows[1], &rows).unwrap_err(), "ambiguous_chunk_family");
        }
    }

    fn row(id: &str, name: &str, project: &str, blocked: &str) -> Row {
        [
            ("task_id", id),
            ("task_name", name),
            ("project", project),
            ("blocked_by", blocked),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
    }

    #[test]
    fn consecutive_gaps_and_no_unfinished_successor() {
        let mut rows = [chunk("T1", 1), chunk("T4", 4)];
        assert_eq!(next(&rows[0], &rows).unwrap().unwrap().id, "T4");
        rows[1].done = true;
        assert_eq!(next(&rows[0], &rows).unwrap(), None);
        assert_eq!(next(&rows[1], &rows).unwrap(), None);
    }

    #[test]
    fn parser_preserves_original_total_and_ignores_ordinary_names() {
        let source = row("T3", " Read spec (3/4) ", " alpha ", "");
        let parsed = parse(&source).unwrap();
        assert_eq!(
            (
                parsed.base.as_str(),
                parsed.index,
                parsed.total,
                parsed.project.as_str()
            ),
            ("Read spec", 3, 4, "alpha")
        );
        assert_eq!(parse(&row("T9", "Read spec", "alpha", "")), None);
    }

    #[test]
    fn cross_key_same_title_edges_fail_in_both_directions() {
        for (name, project) in [("Read spec (1/4)", "beta"), ("Read spec (1/5)", "alpha")] {
            for incoming in [false, true] {
                let rows = [
                    row(
                        "T1",
                        "Read spec (1/4)",
                        "alpha",
                        if incoming { "" } else { "T9" },
                    ),
                    row("T3", "Read spec (3/4)", "alpha", ""),
                    row("T9", name, project, if incoming { "T1" } else { "" }),
                ];
                assert_eq!(
                    validate(&parse(&rows[0]).unwrap(), &rows).unwrap_err(),
                    "ambiguous_chunk_family"
                );
            }
        }
    }

    #[test]
    fn unrelated_title_dependencies_and_other_family_duplicates_do_not_conflict() {
        let rows = [
            row("T1", "Read spec (1/4)", "alpha", "T9"),
            row("T3", "Read spec (3/4)", "alpha", "T9"),
            row("T9", "Write report (1/4)", "alpha", ""),
            row("T10", "Write report (1/4)", "alpha", ""),
        ];
        assert_eq!(
            successor(&rows[0], &rows).unwrap().unwrap()["task_id"],
            "T3"
        );
    }
}
