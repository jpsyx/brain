//! Compose deletion effects from one immutable snapshot, without performing IO.

use std::collections::{BTreeMap, BTreeSet};

use super::model::{FileEdit, Options, Problem, RemovalPlan};
use super::snapshot::{CsvSnapshot, Snapshot};
use crate::tasks::complete::{CsvFile, Row, field, normalize_id, parse_date, serialize_csv};

/// Return a reviewable preview, retaining blockers but withholding all effects.
pub(crate) fn plan(snapshot: &Snapshot, targets: &[String], options: &Options) -> RemovalPlan {
    plan_with_maintenance(snapshot, targets, options, None)
}

pub(crate) fn plan_with_maintenance(
    snapshot: &Snapshot,
    targets: &[String],
    options: &Options,
    maintenance: Option<super::maintenance::MaintenanceIntent>,
) -> RemovalPlan {
    let mut preview = RemovalPlan {
        maintenance,
        fingerprint: String::new(),
        targets: Vec::new(),
        options: options.clone(),
        edits: Vec::new(),
        external: Vec::new(),
        successors: BTreeMap::new(),
        blockers: Vec::new(),
        warnings: snapshot.warnings.clone(),
    };
    match snapshot.selected_identities(targets, options) {
        Ok(selected) => preview.targets = selected,
        Err(problem) => preview.blockers.push(problem),
    }
    if preview.blockers.is_empty() {
        let validation = preview.maintenance.as_ref().map_or(Ok(()), |intent| {
            super::maintenance::validate(snapshot, &preview.targets, options, intent)
        });
        if let Err(problem) = validation.and_then(|()| compose(snapshot, &mut preview)) {
            preview.blockers.push(problem);
        }
    }
    match super::fingerprint::digest(&snapshot.canonical_for(
        options,
        &preview.targets,
        preview.maintenance.as_ref(),
    )) {
        Ok(fingerprint) => preview.fingerprint = fingerprint,
        Err(error) => preview.blockers.push(Problem::new(
            "fingerprint_failed",
            &error.to_string(),
            serde_json::json!({}),
        )),
    }
    if !preview.blockers.is_empty() {
        preview.edits.clear();
        preview.external.clear();
        preview.successors.clear();
    }
    preview
}

pub(super) fn key(value: &str) -> String {
    normalize_id(value.trim()).unwrap_or_else(|_| value.trim().into())
}

fn prerequisites(row: &Row) -> BTreeSet<String> {
    field(row, "blocked_by")
        .split('|')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(key)
        .collect()
}

fn compose(snapshot: &Snapshot, preview: &mut RemovalPlan) -> Result<(), Problem> {
    let removed: BTreeSet<_> = preview
        .targets
        .iter()
        .map(|identity| key(&identity.task_id))
        .collect();
    let originals: Vec<_> = snapshot
        .tasks
        .parsed
        .rows
        .iter()
        .chain(&snapshot.habits.parsed.rows)
        .collect();
    let graph = originals
        .iter()
        .map(|row| (key(&field(row, "task_id")), prerequisites(row)))
        .collect();
    let replacements = super::maintenance::replacements(preview.maintenance.as_ref());
    let rewritten = if replacements.is_empty() {
        super::dependencies::rewrite(&graph, &removed, preview.options.dependents)?
    } else {
        super::dependencies::substitute(
            &graph,
            &replacements.iter().map(|(a, b)| (key(a), key(b))).collect(),
        )?
    };
    let display_ids = originals
        .iter()
        .map(|row| (key(&field(row, "task_id")), field(row, "task_id")))
        .collect();
    let unfinished: BTreeSet<_> = originals
        .iter()
        .filter(|row| {
            !removed.contains(&key(&field(row, "task_id"))) && field(row, "status").trim() != "done"
        })
        .map(|row| key(&field(row, "task_id")))
        .collect();
    let exact_removed = preview
        .targets
        .iter()
        .map(|identity| identity.task_id.clone())
        .collect();
    let mut tasks = transform(
        &snapshot.tasks,
        &removed,
        &exact_removed,
        &graph,
        &rewritten,
        &display_ids,
        &replacements,
    );
    let mut habits = transform(
        &snapshot.habits,
        &removed,
        &exact_removed,
        &graph,
        &rewritten,
        &display_ids,
        &replacements,
    );
    preview.successors = handoffs(
        &snapshot.tasks.parsed.rows,
        &mut tasks.rows,
        &removed,
        &unfinished,
        &snapshot.execution_date,
    )?;
    let projects = super::maintenance::project_edits(
        snapshot,
        &preview.targets,
        &exact_removed,
        preview.maintenance.as_ref(),
    )?;
    preview.external =
        super::external_policy::plan(snapshot, &removed, preview.options.linked_issue)?;
    for (relative, source, csv) in [
        ("tasks/tasks.csv", &snapshot.tasks, &mut tasks),
        ("tasks/habits.csv", &snapshot.habits, &mut habits),
    ] {
        touch_survivors(source, csv, &snapshot.execution_date, relative)?;
        if csv.rows != source.parsed.rows {
            let after = serialize_csv(csv).map_err(|error| {
                Problem::new(
                    "invalid_task_csv",
                    &error.to_string(),
                    serde_json::json!({"path":relative}),
                )
            })?;
            if source.before.as_deref() != Some(after.as_slice()) {
                preview.edits.push(FileEdit {
                    relative: relative.into(),
                    before: source.before.clone(),
                    after,
                });
            }
        }
    }
    preview.edits.extend(projects);
    preview.edits.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(())
}

fn transform(
    source: &CsvSnapshot,
    removed: &BTreeSet<String>,
    exact_removed: &BTreeSet<String>,
    graph: &super::dependencies::Graph,
    rewritten: &super::dependencies::Graph,
    display_ids: &BTreeMap<String, String>,
    replacements: &BTreeMap<String, String>,
) -> CsvFile {
    let rows = source
        .parsed
        .rows
        .iter()
        .filter(|row| !removed.contains(&key(&field(row, "task_id"))))
        .map(|original| {
            let id = key(&field(original, "task_id"));
            let mut row = original.clone();
            if graph[&id] != rewritten[&id] {
                row.insert(
                    "blocked_by".into(),
                    serialize_prerequisites(original, &rewritten[&id], display_ids),
                );
            }
            let references = field(&row, "see_also");
            let stripped = if replacements.is_empty() {
                super::references::strip_items(&references, exact_removed)
            } else {
                super::references::replace_tokens(
                    &references.split('|').map(str::to_owned).collect::<Vec<_>>(),
                    replacements,
                    true,
                )
                .join("|")
            };
            if stripped != references {
                row.insert("see_also".into(), stripped);
            }
            row
        })
        .collect();
    CsvFile {
        header: source.parsed.header.clone(),
        rows,
    }
}

fn serialize_prerequisites(
    original: &Row,
    rewritten: &BTreeSet<String>,
    display_ids: &BTreeMap<String, String>,
) -> String {
    let mut existing = BTreeMap::new();
    for token in field(original, "blocked_by")
        .split('|')
        .filter(|token| !token.trim().is_empty())
    {
        existing
            .entry(key(token))
            .or_insert_with(|| token.to_owned());
    }
    let tokens = rewritten
        .iter()
        .map(|id| {
            existing
                .get(id)
                .or_else(|| display_ids.get(id))
                .unwrap_or(id)
                .clone()
        })
        .collect();
    super::dependencies::ordered_prerequisites(&tokens).join("|")
}

fn touch_survivors(
    source: &CsvSnapshot,
    csv: &mut CsvFile,
    today: &str,
    relative: &str,
) -> Result<(), Problem> {
    let originals: BTreeMap<_, _> = source
        .parsed
        .rows
        .iter()
        .map(|row| (field(row, "task_id"), row))
        .collect();
    for row in &mut csv.rows {
        let id = field(row, "task_id");
        if row != originals[&id] {
            if !csv.header.iter().any(|column| column == "last_touched") {
                return Err(Problem::new(
                    "missing_last_touched_column",
                    "surviving row edits require the original last_touched column",
                    serde_json::json!({"csv":relative, "task_id":id}),
                ));
            }
            row.insert("last_touched".into(), today.into());
        }
    }
    Ok(())
}

fn handoffs(
    originals: &[Row],
    survivors: &mut [Row],
    removed: &BTreeSet<String>,
    unfinished: &BTreeSet<String>,
    today: &str,
) -> Result<BTreeMap<String, String>, Problem> {
    use crate::tasks::chunk_family;
    // Resolve display aliases in family evidence without changing stored rows.
    let evidence: Vec<_> = originals
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row.insert("task_id".into(), key(&field(&row, "task_id")));
            row.insert(
                "blocked_by".into(),
                prerequisites(&row)
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join("|"),
            );
            row
        })
        .collect();
    let mut anchors: Vec<_> = evidence
        .iter()
        .filter(|row| removed.contains(&field(row, "task_id")))
        .collect();
    anchors.sort_by_key(|row| key(&field(row, "task_id")));
    let mut successors = BTreeMap::new();
    for row in anchors {
        let Some(anchor) = chunk_family::parse(row) else {
            continue;
        };
        let chunks = chunk_family::validate(&anchor, &evidence).map_err(|code| {
            Problem::new(
                &code,
                "removed chunk has conflicting chunk-family evidence",
                serde_json::json!({"task_id":anchor.id}),
            )
        })?;
        let chunks: Vec<_> = chunks
            .into_iter()
            .filter(|chunk| !removed.contains(&chunk.id))
            .collect();
        let Some(next) = chunk_family::next(&anchor, &chunks).map_err(|code| {
            Problem::new(
                &code,
                "removed chunk has ambiguous successors",
                serde_json::json!({"task_id":anchor.id}),
            )
        })?
        else {
            continue;
        };
        let Some(index) = survivors
            .iter()
            .position(|row| key(&field(row, "task_id")) == next.id)
        else {
            continue;
        };
        let candidate = &survivors[index];
        let status = field(candidate, "status");
        let start = field(candidate, "start_date");
        let visible = parse_date(&start)
            .is_none_or(|date| parse_date(today).is_some_and(|today| date <= today));
        let blocked = prerequisites(candidate)
            .iter()
            .any(|id| unfinished.contains(id));
        let types = field(candidate, "task_type");
        if !matches!(status.trim(), "not_started" | "in_progress") || !visible || blocked {
            continue;
        }
        let successor = field(candidate, "task_id");
        if field(row, "task_type")
            .split('|')
            .any(|kind| kind.trim() == "mit")
            && !types.split('|').any(|kind| kind.trim() == "mit")
        {
            let promoted = if types.is_empty() {
                "mit".into()
            } else {
                format!("{types}|mit")
            };
            survivors[index].insert("task_type".into(), promoted);
        }
        let original_id = originals
            .iter()
            .find(|original| key(&field(original, "task_id")) == anchor.id)
            .map(|original| field(original, "task_id"))
            .unwrap_or(anchor.id);
        successors.insert(original_id, successor);
    }
    Ok(successors)
}
