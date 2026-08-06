//! Replayable task-id bridge for a legacy machine joining a current remote.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::sync::counters::{COUNTERS, parse_counter, reconcile_counter_value};
use crate::sync::csv_merge::{
    SchemaStatus, Table, merge, parse, remote_schema_status, schema_status, serialize,
    validate_for_merge,
};

const CSVS: [(&str, &str); 2] = [
    ("tasks/tasks.csv", "tasks.csv"),
    ("tasks/habits.csv", "habits.csv"),
];

struct Generation {
    path: PathBuf,
    merged: Table,
}

/// Reconcile current remote rows into a legacy local generation without
/// publishing anything. Replaying after a crash produces the same bytes.
pub fn join_legacy_to_current_with_transport(
    paths: &crate::workspace::WorkspacePaths,
    root: &Path,
    remote_schema: &str,
    mut fetch: impl FnMut(&str) -> Option<String>,
) -> Result<()> {
    let _task_owner =
        crate::tasks::store_lock::TaskStoreOwner::acquire_path(&paths.task_store_lock())?;
    let local_schema_path = root.join("tasks/SCHEMA.json");
    let local_schema = fs::read_to_string(&local_schema_path)
        .with_context(|| format!("reading legacy task schema {}", local_schema_path.display()))?;
    if schema_status(Some(&local_schema))? != SchemaStatus::Legacy {
        bail!("legacy-to-current join requires a legacy local task schema");
    }
    if remote_schema_status(Some(remote_schema))? != SchemaStatus::Current {
        bail!("legacy-to-current join requires a current remote task schema");
    }

    let mut local_tables = Vec::with_capacity(CSVS.len() * 2);
    let mut remote_current_tables = Vec::with_capacity(CSVS.len());
    let mut remote_legacy_tables = Vec::with_capacity(CSVS.len());
    let mut paths_to_write = Vec::with_capacity(CSVS.len());
    for (relative, baseline_name) in CSVS {
        let local_path = root.join(relative);
        let local_text = fs::read_to_string(&local_path)
            .with_context(|| format!("reading legacy join input {}", local_path.display()))?;
        let baseline_text =
            fs::read_to_string(crate::sync::csv_sync::baseline_path(paths, baseline_name))
                .unwrap_or_default();
        let remote_text = fetch(relative)
            .ok_or_else(|| anyhow!("current remote is missing required {relative}"))?;
        local_tables.push(parse(&baseline_text, SchemaStatus::Legacy)?);
        local_tables.push(parse(&local_text, SchemaStatus::Legacy)?);
        remote_current_tables.push(parse(&remote_text, SchemaStatus::Current)?);
        remote_legacy_tables.push(parse(&remote_text, SchemaStatus::Legacy)?);
        paths_to_write.push(local_path);
    }
    validate_for_merge(
        Some(&local_schema),
        &local_tables.iter().collect::<Vec<_>>(),
    )?;
    validate_for_merge(
        Some(remote_schema),
        &remote_current_tables.iter().collect::<Vec<_>>(),
    )?;

    let mut generations = Vec::with_capacity(CSVS.len());
    for (index, path) in paths_to_write.into_iter().enumerate() {
        let mut merged = merge(
            &local_tables[index * 2],
            &local_tables[index * 2 + 1],
            &remote_legacy_tables[index],
        )
        .0;
        preserve_remote_uuids(&mut merged, &remote_current_tables[index])?;
        generations.push(Generation { path, merged });
    }
    let floors = generations
        .iter()
        .zip(['T', 'H'])
        .map(|(generation, prefix)| {
            crate::sync::counters::counter_floor_from_csvs(
                &serialize(&generation.merged),
                "",
                prefix,
            )
            .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    for generation in &generations {
        replace(&generation.path, serialize(&generation.merged).as_bytes())?;
    }
    for (relative, floor) in COUNTERS.into_iter().zip(floors) {
        let local_path = root.join(relative);
        let local = fs::read_to_string(&local_path)
            .ok()
            .and_then(|text| parse_counter(&text));
        let remote = fetch(relative).and_then(|text| parse_counter(&text));
        if let Some(value) = reconcile_counter_value(local, remote, floor) {
            replace(&local_path, format!("{value}\n").as_bytes())?;
        }
    }
    Ok(())
}

pub(crate) fn join_legacy_to_current(
    context: &crate::workspace::CommandContext,
    remote: &crate::sync::remote::Remote,
    remote_schema: &str,
) -> Result<()> {
    join_legacy_to_current_with_transport(
        context.workspace.paths(),
        context.workspace.root(),
        remote_schema,
        |relative| fetch_remote_csv(context.workspace.paths(), remote, relative),
    )
}

fn fetch_remote_csv(
    paths: &crate::workspace::WorkspacePaths,
    remote: &crate::sync::remote::Remote,
    relative: &str,
) -> Option<String> {
    let temporary_dir = paths.sync_dir().join("tmp");
    fs::create_dir_all(&temporary_dir).ok()?;
    let temporary = temporary_dir.join(format!(
        "legacy-join-fetch-{}",
        crate::workspace::WorkspaceId::new()
    ));
    let (success, _) = crate::sync::run::run_rclone_capture(
        &remote.env,
        &[
            "copyto".to_owned(),
            crate::sync::csv_sync::remote_csv_arg(&remote.arg, relative),
            temporary.to_string_lossy().into_owned(),
        ],
    );
    let result = success
        .then(|| fs::read_to_string(&temporary).ok())
        .flatten();
    let _ = fs::remove_file(temporary);
    result
}

fn preserve_remote_uuids(merged: &mut Table, remote: &Table) -> Result<()> {
    let merged_uuid = merged
        .column("task_uuid")
        .ok_or_else(|| anyhow!("legacy join output has no task_uuid column"))?;
    let remote_display = remote
        .column("task_id")
        .ok_or_else(|| anyhow!("current remote task CSV has no task_id column"))?;
    for (uuid, remote_row) in &remote.rows {
        let display = remote_row.get(remote_display).map_or("", String::as_str);
        if let Some(merged_row) = merged.rows.get_mut(display) {
            merged_row[merged_uuid].clone_from(uuid);
        }
    }
    Ok(())
}

fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("legacy join output has no parent: {}", path.display()))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("csv"),
        crate::workspace::WorkspaceId::new()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("creating legacy join temporary {}", temporary.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("writing legacy join temporary {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("syncing legacy join temporary {}", temporary.display()))?;
        fs::rename(&temporary, path)
            .with_context(|| format!("publishing legacy join output {}", path.display()))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("syncing legacy join directory {}", parent.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
