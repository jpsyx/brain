use super::{row, today};
use crate::tasks::agenda::{RemovalAgendaChange, Snapshot, sync_removal_markdown};

#[test]
fn absent_removed_chunk_uses_explicit_successor_and_retained_before_image() {
    let before = b"task_id,task_name,status\nT2,Read spec (2/4),not_started\nT3,Read spec (3/4),not_started\n";
    let edit = crate::tasks::removal::model::FileEdit {
        relative: "tasks/tasks.csv".into(),
        before: Some(before.to_vec()),
        after: b"task_id,task_name,status\nT3,Read spec (3/4),not_started\n".to_vec(),
    };
    let tasks = [row(&[
        ("task_id", "T3"),
        ("task_name", "Read spec (3/4)"),
        ("status", "not_started"),
        ("estimated_duration", "25"),
    ])];
    let text = "## ❗ MIT\n\n- [ ] ❗ **T2** Read spec (2/4)\n\n## Suggested order\n\n1. [ ] 09:00 | **T2** Read spec (2/4)\n";
    let out = sync_removal_markdown(
        text,
        &[RemovalAgendaChange {
            removed_id: "T2".into(),
            successor_id: Some("T3".into()),
        }],
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    )
    .unwrap();
    assert!(edit.before.unwrap().windows(2).any(|bytes| bytes == b"T2"));
    assert!(!out.contains("**T2**"));
    assert_eq!(out.matches("**T3**").count(), 2, "{out}");
    assert!(
        out.contains("1. [ ] 09:00 | **T3** Read spec (3/4) (25m)"),
        "{out}"
    );
}

fn change(removed: &str, successor: Option<&str>) -> RemovalAgendaChange {
    RemovalAgendaChange {
        removed_id: removed.into(),
        successor_id: successor.map(str::to_owned),
    }
}

fn successor() -> crate::tasks::complete::Row {
    row(&[
        ("task_id", "T3"),
        ("task_name", "Read spec (3/4)"),
        ("status", "not_started"),
    ])
}

#[test]
fn batch_chooses_earliest_original_slot_independent_of_effect_order() {
    let text = "# Day\n\n**Load:** keep\n\n## ❗ MIT\n\n- [ ] ❗ **T1** First\n- [ ] ❗ **T2** Second\n- [ ] ❗ **T1** Duplicate\n\n## Suggested order\n\n1. [ ] 09:00 | **T1** First\n2. [ ] 10:00 | **T2** Second\n3. [ ] 11:00 | **T1** Duplicate\n4. [ ] 12:00 | **T9** Unrelated\n\n## Cut order\n\n1. **T1** First\n2. **T2** Second\n3. **T9** Unrelated\n\n## Notes\n\nBare T1 stays.\n\n## Appendix <!-- brain:optional-content -->\n\n**T1** reference stays.\n";
    let tasks = [successor()];
    let snapshot = Snapshot {
        tasks: &tasks,
        habits: &[],
    };
    let forward = [change("T1", Some("T3")), change("T2", Some("T3"))];
    let reverse = [change("T2", Some("T3")), change("T1", Some("T3"))];
    let out = sync_removal_markdown(text, &forward, &snapshot, today()).unwrap();
    assert_eq!(
        out,
        sync_removal_markdown(text, &reverse, &snapshot, today()).unwrap()
    );
    assert_eq!(out.matches("**T3**").count(), 2, "{out}");
    assert!(out.contains("1. [ ] 09:00 | **T3**"), "{out}");
    assert!(out.contains("2. [ ] 12:00 | **T9**"), "{out}");
    assert!(out.contains("## Cut order\n\n1. **T9**"), "{out}");
    assert!(out.ends_with("## Notes\n\nBare T1 stays.\n\n## Appendix <!-- brain:optional-content -->\n\n**T1** reference stays.\n"));
}

#[test]
fn supplied_successor_is_drop_only_when_currently_ineligible_without_fallback() {
    let text = "## ❗ MIT\n\n- [ ] ❗ **T2** Removed\n\n## Suggested order\n\n1. [ ] 09:00 | **T2** Removed\n";
    let later = row(&[
        ("task_id", "T4"),
        ("task_name", "Read spec (4/4)"),
        ("status", "not_started"),
    ]);
    for cells in [
        vec![("status", "waiting")],
        vec![("status", "backlog")],
        vec![("status", "done")],
        vec![("start_date", "2026-08-25")],
        vec![("blocked_by", "h002 | T9")],
    ] {
        let mut next = successor();
        for (key, value) in cells {
            next.insert(key.into(), value.into());
        }
        let tasks = [
            next,
            later.clone(),
            row(&[("task_id", "T9"), ("status", "done")]),
        ];
        let habits = [row(&[("task_id", "H2"), ("status", "not_started")])];
        let out = sync_removal_markdown(
            text,
            &[change("T2", Some("T3"))],
            &Snapshot {
                tasks: &tasks,
                habits: &habits,
            },
            today(),
        )
        .unwrap();
        assert!(
            !out.contains("**T2**") && !out.contains("**T3**") && !out.contains("**T4**"),
            "{out}"
        );
    }
    for target in [None, Some("T99")] {
        let out = sync_removal_markdown(
            text,
            &[change("T2", target)],
            &Snapshot {
                tasks: &[successor(), later.clone()],
                habits: &[],
            },
            today(),
        )
        .unwrap();
        assert!(
            !out.contains("**T2**") && !out.contains("**T3**") && !out.contains("**T4**"),
            "{out}"
        );
    }
}

#[test]
fn eligible_successor_allows_future_due_and_done_blockers() {
    let mut next = successor();
    next.insert("status".into(), "in_progress".into());
    next.insert("due_date".into(), "2027-01-01".into());
    next.insert("blocked_by".into(), "h002|T9".into());
    let tasks = [next, row(&[("task_id", "T9"), ("status", "done")])];
    let habits = [row(&[("task_id", "H2"), ("status", "done")])];
    let out = sync_removal_markdown(
        "## ❗ MIT\n\n- [ ] ❗ **T2** Removed\n",
        &[change("T2", Some("T3"))],
        &Snapshot {
            tasks: &tasks,
            habits: &habits,
        },
        today(),
    )
    .unwrap();
    assert!(out.contains("**T3**"), "{out}");
}

#[test]
fn existing_successor_callouts_win_and_are_preserved() {
    let text = "## ❗ MIT\n\n- [ ] ❗ **T2** Removed\n- [ ] ❗ **T2** Duplicate\n- [ ] ❗ **T3** Keep original wording\n\n## Suggested order\n\n1. [ ] 09:00 | **T2** Removed\n2. [ ] 10:00 | **T3** Keep original wording\n3. [ ] 11:00 | **T3** Keep existing duplicate\n";
    let out = sync_removal_markdown(
        text,
        &[change("T2", Some("T3"))],
        &Snapshot {
            tasks: &[successor()],
            habits: &[],
        },
        today(),
    )
    .unwrap();
    assert!(!out.contains("**T2**"));
    assert_eq!(out.matches("**T3**").count(), 3);
    assert!(out.contains("1. [ ] 10:00 | **T3** Keep original wording"));
    assert!(out.contains("2. [ ] 11:00 | **T3** Keep existing duplicate"));
}

#[test]
fn removal_rederives_habits_and_completed_history_from_current_rows() {
    let text = "## ✅ Completed today\n\n| ✅ **T2** Removed history | ✅ **H1** Removed habit |\n\n## 🔁 Today's habits\n\n| ✅ **H1** Removed habit | |\n";
    let tasks = [row(&[
        ("task_id", "T9"),
        ("status", "done"),
        ("completed_date", "2026-08-24"),
        ("task_name", "Kept history"),
    ])];
    let out = sync_removal_markdown(
        text,
        &[change("T2", None), change("H1", None)],
        &Snapshot {
            tasks: &tasks,
            habits: &[],
        },
        today(),
    )
    .unwrap();
    assert!(!out.contains("**T2**") && !out.contains("**H1**") && !out.contains("## 🔁"));
    assert!(out.contains("✅ **T9** Kept history"));
}
