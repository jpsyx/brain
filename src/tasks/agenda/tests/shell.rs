//! The filesystem shell, and the composition BR-19 was filed about: a native
//! completion must leave the day's agenda accurate.

use std::path::Path;

use super::today;
use crate::tasks::agenda::{Outcome, Targets};

const TASKS_CSV: &str = "\
task_id,task_name,status,task_type,estimated_duration,completed_date,last_touched
T535,Fix the sync,not_started,mit,45,,2026-08-20
T536,Write the docs,not_started,,30,,2026-08-20
";

const HABITS_CSV: &str = "\
task_id,task_name,status,due_date,ideal_time,recur_interval,recur_unit,completed_date,last_touched
H304,Walk the dog,not_started,2026-08-24,07:00,1,days,,2026-08-20
";

const AGENDA: &str = "\
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

struct Fixture {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    targets: Targets,
}

fn fixture(agenda: Option<&str>) -> Fixture {
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

fn actor(root: &Path) -> crate::actor::ActorContext {
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
fn fake_renderer(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("markdown-to-pdf");
    std::fs::write(&path, "#!/bin/sh\ncp \"$1\" \"$3\"\n").expect("write renderer");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod renderer");
    path
}

fn complete(fixture: &Fixture, id: &str) -> Outcome {
    crate::tasks::complete::complete_and_sync_in_root(
        &fixture.root,
        &fixture.targets,
        id,
        today(),
        &actor(&fixture.root),
    )
    .expect("completion")
    .1
}

#[test]
fn native_completion_rewrites_the_agenda_it_invalidated() {
    let fixture = fixture(Some(AGENDA));

    let outcome = complete(&fixture, "T535");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    let synced = std::fs::read_to_string(&fixture.targets.markdown).expect("read agenda");
    assert_eq!(
        synced,
        "\
# Monday 2026-08-24

**Load:** 2 tasks, 1 habit
**Bottom line:** ship the sync.

## ❗ Most important


## Suggested order

1. [ ] 10:00 | **T536** Write the docs (30m)

## Cut order

1. **T536** Write the docs

## 🔁 Today's habits

|  |  |
|---|---|
| ◻ **H304** Walk the dog |  |

## ✅ Completed today

|  |  |
|---|---|
| ✅ **T535** Fix the sync |  |

"
    );
}

#[test]
fn a_completed_habit_moves_into_both_snapshots() {
    let fixture = fixture(Some(AGENDA));

    let outcome = complete(&fixture, "H304");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    let synced = std::fs::read_to_string(&fixture.targets.markdown).expect("read agenda");
    assert!(synced.contains("| ✅ **H304** Walk the dog |"), "{synced}");
    // Completion spawned tomorrow's occurrence, which is not due today.
    assert!(!synced.contains("◻ **H305**"), "{synced}");
    // The task plan is untouched by a habit completion.
    assert!(
        synced.contains("1. [ ] 09:00 | **T535** Fix the sync (45m)"),
        "{synced}"
    );
}

#[test]
fn no_agenda_for_the_day_is_a_clean_no_op() {
    let fixture = fixture(None);

    let outcome = complete(&fixture, "T535");

    assert_eq!(outcome, Outcome::NoAgenda);
    // The CSV mutation still happened — the agenda is downstream, never a gate.
    let tasks = std::fs::read_to_string(fixture.targets.tasks_dir.join("tasks.csv"))
        .expect("read tasks.csv");
    assert!(tasks.contains("T535,Fix the sync,done"), "{tasks}");
}

#[test]
fn re_syncing_an_already_synced_agenda_changes_nothing() {
    let fixture = fixture(Some(AGENDA));
    assert_eq!(complete(&fixture, "T535"), Outcome::Updated { pdf: false });

    // Idempotent: the second pass finds the file already accurate and leaves
    // it (and any printable) alone.
    let outcome = crate::tasks::agenda::sync_targets(
        &fixture.targets,
        "T535",
        crate::tasks::agenda::Action::Done,
        today(),
    );

    assert_eq!(outcome, Outcome::Unchanged);
}

#[test]
fn an_existing_printable_is_regenerated_from_comment_free_markdown() {
    let mut fixture = fixture(Some(&format!(
        "{AGENDA}\n## Appendix <!-- brain:optional-content -->\n\nBaked content.\n"
    )));
    let dir = fixture.targets.pdf.parent().expect("pdf dir").to_path_buf();
    fixture.targets.renderer = Some(fake_renderer(&dir));
    std::fs::write(&fixture.targets.pdf, "stale printable").expect("seed pdf");

    let outcome = complete(&fixture, "T535");

    assert_eq!(outcome, Outcome::Updated { pdf: true });
    let printed = std::fs::read_to_string(&fixture.targets.pdf).expect("read pdf");
    assert!(!printed.contains("<!--"), "{printed}");
    assert!(printed.contains("## Appendix\n"), "{printed}");
    assert!(printed.contains("Baked content."), "{printed}");
    // The marker must survive in the source, because the appendix baker greps
    // for it to stay idempotent.
    let source = std::fs::read_to_string(&fixture.targets.markdown).expect("read agenda");
    assert!(
        source.contains("<!-- brain:optional-content -->"),
        "{source}"
    );
    // The staged render copy is cleaned up.
    assert!(
        !fixture
            .targets
            .markdown
            .with_extension("render.md")
            .exists()
    );
}

#[test]
fn no_printable_on_disk_means_no_regen() {
    let mut fixture = fixture(Some(AGENDA));
    let dir = fixture.targets.pdf.parent().expect("pdf dir").to_path_buf();
    fixture.targets.renderer = Some(fake_renderer(&dir));

    let outcome = complete(&fixture, "T535");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    assert!(!fixture.targets.pdf.exists());
}
