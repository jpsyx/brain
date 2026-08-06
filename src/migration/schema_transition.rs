//! Schema-last publication for the coordinated task identity cutover.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};

const TASKS: &str = "tasks/tasks.csv";
const HABITS: &str = "tasks/habits.csv";
const SCHEMA: &str = "tasks/SCHEMA.json";

pub fn publish_task_schema_transition_with_transport(
    paths: &crate::workspace::WorkspacePaths,
    root: &Path,
    remote_schema: Option<&str>,
    mut publish: impl FnMut(&str, &[u8]) -> bool,
) -> Result<()> {
    let inspection = crate::tasks::schema::inspect_inactive(root)?;
    if !inspection.current {
        bail!("task schema transition requires current local CSV and schema state");
    }
    crate::sync::csv_merge::remote_schema_status(remote_schema)?;
    let tasks = read(root, TASKS)?;
    let habits = read(root, HABITS)?;
    let schema = read(root, SCHEMA)?;

    for (relative, bytes) in [(TASKS, tasks.as_slice()), (HABITS, habits.as_slice())] {
        if !publish(relative, bytes) {
            bail!("task schema transition could not publish {relative}");
        }
    }

    write_baseline(&paths.sync_csv_baselines().join("tasks.csv"), &tasks)?;
    write_baseline(&paths.sync_csv_baselines().join("habits.csv"), &habits)?;

    if !publish(SCHEMA, &schema) {
        bail!("task schema transition could not publish {SCHEMA}");
    }
    Ok(())
}

pub(crate) fn publish_task_schema_transition(
    context: &crate::workspace::CommandContext,
    config: &crate::sync::config::SyncConfig,
) -> Result<()> {
    let remote = crate::sync::remote::build_remote(config);
    let verified = crate::sync::identity::require_remote_identity(
        context.workspace.root(),
        context.workspace.id(),
        &remote,
    )?;
    let temporary_dir = context.workspace.paths().sync_dir().join("tmp");
    fs::create_dir_all(&temporary_dir).with_context(|| {
        format!(
            "creating task schema transition directory {}",
            temporary_dir.display()
        )
    })?;
    let remote_schema = crate::sync::csv_sync::fetch_remote_task_schema(
        context.workspace.paths(),
        verified.remote(),
    )?;
    publish_task_schema_transition_with_transport(
        context.workspace.paths(),
        context.workspace.root(),
        remote_schema.as_deref(),
        |relative, bytes| publish_remote(&temporary_dir, verified.remote(), relative, bytes),
    )
}

fn publish_remote(
    temporary_dir: &Path,
    remote: &crate::sync::remote::Remote,
    relative: &str,
    bytes: &[u8],
) -> bool {
    let tag = relative.replace('/', "_");
    let temporary = temporary_dir.join(format!(
        "schema-transition-{}-{tag}",
        crate::workspace::WorkspaceId::new()
    ));
    let written = fs::write(&temporary, bytes).is_ok();
    let published = written
        && crate::sync::run::run_rclone_capture(
            &remote.env,
            &[
                "copyto".to_owned(),
                temporary.to_string_lossy().into_owned(),
                crate::sync::csv_sync::remote_csv_arg(&remote.arg, relative),
            ],
        )
        .0;
    let _ = fs::remove_file(temporary);
    published
}

fn read(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = root.join(relative);
    fs::read(&path)
        .with_context(|| format!("reading task schema transition input {}", path.display()))
}

fn write_baseline(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("task CSV baseline has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating task CSV baseline directory {}", parent.display()))?;
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
            .with_context(|| {
                format!(
                    "creating task CSV baseline temporary {}",
                    temporary.display()
                )
            })?;
        file.write_all(bytes).with_context(|| {
            format!(
                "writing task CSV baseline temporary {}",
                temporary.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "syncing task CSV baseline temporary {}",
                temporary.display()
            )
        })?;
        fs::rename(&temporary, path).with_context(|| {
            format!(
                "replacing task CSV baseline {} from {}",
                path.display(),
                temporary.display()
            )
        })?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("syncing task CSV baseline directory {}", parent.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
