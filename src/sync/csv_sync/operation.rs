use std::path::{Path, PathBuf};

use crate::sync::args::Direction;
use crate::sync::csv_merge::{Table, merge, parse, schema_status, serialize, validate_for_merge};

use super::metadata::{MetadataPublishError, prepare_project_metadata, publish_project_metadata};
use super::{CSVS, CsvMergeOutcome, CsvSyncError, CsvSyncResult, DisplayIdFloors, baseline_path};

struct CsvGeneration {
    relative: &'static str,
    local: PathBuf,
    baseline: PathBuf,
    baseline_text: String,
    local_text: String,
    remote_text: String,
    base: Table,
    local_table: Table,
    remote_table: Table,
}

struct PreparedCsv {
    generation: CsvGeneration,
    merged: Table,
    text: String,
    outcome: CsvMergeOutcome,
}

fn display_floor(table: &Table, prefix: char) -> u32 {
    let Some(index) = table.header.iter().position(|column| column == "task_id") else {
        return 0;
    };
    table
        .rows
        .values()
        .filter_map(|row| row.get(index))
        .filter_map(|display| display.trim().strip_prefix(prefix))
        .filter_map(|number| number.parse::<u32>().ok())
        .max()
        .and_then(|maximum| maximum.checked_add(1))
        .unwrap_or(0)
}

fn write_checked(path: &Path, text: &str) -> Result<(), CsvSyncError> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|error| {
            CsvSyncError::LocalWrite(format!("creating {}: {error}", directory.display()))
        })?;
    }
    std::fs::write(path, text)
        .map_err(|error| CsvSyncError::LocalWrite(format!("writing {}: {error}", path.display())))
}

pub(super) fn sync_csvs_with_transport(
    paths: &crate::workspace::WorkspacePaths,
    root: &Path,
    direction: Direction,
    mut fetch: impl FnMut(&str) -> Option<String>,
    mut push: impl FnMut(&str, &str) -> bool,
) -> Result<CsvSyncResult, CsvSyncError> {
    let manifest = std::fs::read_to_string(root.join("tasks/SCHEMA.json")).ok();
    let schema_status = schema_status(manifest.as_deref())
        .map_err(|error| CsvSyncError::Preflight(format!("{error:#}")))?;
    let mut generations = Vec::with_capacity(CSVS.len());
    for relative in CSVS {
        let local = root.join(relative);
        let name = Path::new(relative)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(relative);
        let baseline = baseline_path(paths, name);
        let baseline_text = std::fs::read_to_string(&baseline).unwrap_or_default();
        let local_text = std::fs::read_to_string(&local).unwrap_or_default();
        let remote_text = fetch(relative).unwrap_or_default();
        let parse_generation = |generation: &str, text: &str| {
            parse(text, schema_status).map_err(|error| {
                CsvSyncError::Preflight(format!("{generation} {relative}: {error}"))
            })
        };
        generations.push(CsvGeneration {
            relative,
            local,
            baseline,
            base: parse_generation("baseline", &baseline_text)?,
            local_table: parse_generation("local", &local_text)?,
            remote_table: parse_generation("remote", &remote_text)?,
            baseline_text,
            local_text,
            remote_text,
        });
    }

    let tables = generations
        .iter()
        .flat_map(|generation| {
            [
                &generation.base,
                &generation.local_table,
                &generation.remote_table,
            ]
        })
        .collect::<Vec<_>>();
    validate_for_merge(manifest.as_deref(), &tables)
        .map_err(|error| CsvSyncError::Preflight(format!("{error:#}")))?;

    let mut prepared = Vec::with_capacity(generations.len());
    for generation in generations {
        let (merged, report) = merge(
            &generation.base,
            &generation.local_table,
            &generation.remote_table,
        );
        let text = serialize(&merged);
        let name = Path::new(generation.relative)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(generation.relative)
            .to_owned();
        prepared.push(PreparedCsv {
            generation,
            merged,
            text,
            outcome: CsvMergeOutcome {
                name,
                added: report.added,
                deleted: report.deleted,
                merged: report.merged,
                soft_conflicts: report.soft_conflicts.len(),
            },
        });
    }

    let merged_tables = prepared
        .iter()
        .map(|csv| csv.merged.clone())
        .collect::<Vec<_>>();
    let metadata = prepare_project_metadata(root, &merged_tables)
        .map_err(|error| CsvSyncError::Preflight(format!("{error:#}")))?;
    let floors = DisplayIdFloors {
        tasks: prepared
            .first()
            .map_or(0, |csv| display_floor(&csv.merged, 'T')),
        habits: prepared
            .get(1)
            .map_or(0, |csv| display_floor(&csv.merged, 'H')),
    };

    for csv in &prepared {
        let generation = &csv.generation;
        if direction != Direction::Push && generation.local_text != csv.text {
            write_checked(&generation.local, &csv.text)?;
        }
        if generation.remote_text != csv.text && !push(generation.relative, &csv.text) {
            return Err(CsvSyncError::RemotePublish(generation.relative.to_owned()));
        }
        if direction != Direction::Push && generation.baseline_text != csv.text {
            write_checked(&generation.baseline, &csv.text)?;
        }
    }
    publish_project_metadata(&metadata, direction != Direction::Push, |relative, text| {
        push(relative, text)
    })
    .map_err(|error| match error {
        MetadataPublishError::Local(message) => CsvSyncError::LocalWrite(message),
        MetadataPublishError::Remote(relative) => CsvSyncError::RemotePublish(relative),
    })?;

    Ok(CsvSyncResult {
        outcomes: prepared.into_iter().map(|csv| csv.outcome).collect(),
        floors,
    })
}
