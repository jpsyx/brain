//! The filesystem shell, and the composition BR-19 was filed about: a native
//! completion must leave the day's agenda accurate.

use super::fixture::{AGENDA, Fixture, actor, fake_renderer, fixture};
use super::today;
use crate::tasks::agenda::Outcome;

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
