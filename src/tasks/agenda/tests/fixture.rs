//! Shared fixture for the agenda-sync tests: a temporary workspace with both
//! CSVs, an agenda, and injectable targets.

use std::path::Path;

use crate::tasks::agenda::Targets;

pub(super) const TASKS_CSV: &str = "\
task_id,task_name,status,task_type,estimated_duration,completed_date,last_touched
T535,Fix the sync,not_started,mit,45,,2026-08-20
T536,Write the docs,not_started,,30,,2026-08-20
";

pub(super) const HABITS_CSV: &str = "\
task_id,task_name,status,due_date,ideal_time,recur_interval,recur_unit,completed_date,last_touched
H304,Walk the dog,not_started,2026-08-24,07:00,1,days,,2026-08-20
";

pub(super) const AGENDA: &str = "\
# Monday 2026-08-24

**Load:** 2 tasks, 1 habit
**Bottom line:** ship the sync.

## ❗ Most important

- [ ] ❗ **T535** Fix the sync (45m)

## Suggested order

1. [ ] 09:00 | **T535** Fix the sync (45m)
2. [ ] 10:00 | **T536** Write the docs (30m)

## Cut order

1. **T536** Write the docs
2. **T535** Fix the sync
";

pub(super) struct Fixture {
    _dir: tempfile::TempDir,
    pub(super) root: std::path::PathBuf,
    pub(super) targets: Targets,
}

pub(super) fn fixture(agenda: Option<&str>) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("brain");
    let tasks_dir = root.join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
    std::fs::write(tasks_dir.join("tasks.csv"), TASKS_CSV).expect("tasks.csv");
    std::fs::write(tasks_dir.join("habits.csv"), HABITS_CSV).expect("habits.csv");
    let markdown = dir.path().join("2026-08-24.md");
    if let Some(agenda) = agenda {
        std::fs::write(&markdown, agenda).expect("agenda");
    }
    Fixture {
        root,
        targets: Targets {
            markdown,
            pdf: dir.path().join("agenda-2026-08-24.pdf"),
            renderer: None,
            tasks_dir,
        },
        _dir: dir,
    }
}

pub(super) fn actor(root: &Path) -> crate::actor::ActorContext {
    let workspace = crate::workspace::WorkspaceContext::new(
        root,
        crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
            .expect("workspace id"),
        crate::workspace::WorkspaceName::parse("legacy").expect("workspace name"),
        root,
        "test-user",
        root,
    )
    .expect("workspace context");
    crate::actor::local_actor(&workspace).expect("actor")
}

/// A stand-in `markdown-to-pdf`: copies its input to `--out`, so the test can
/// read back exactly what the renderer was fed.
pub(super) fn fake_renderer(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("markdown-to-pdf");
    std::fs::write(&path, "#!/bin/sh\ncp \"$1\" \"$3\"\n").expect("write renderer");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod renderer");
    path
}
