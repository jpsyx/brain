
#[test]
fn raw_root_task_mutators_are_not_public_api() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (path, forbidden) in [
        ("src/tasks/complete/mod.rs", "pub fn complete_in_root"),
        ("src/tasks/revive/mod.rs", "pub fn revive_fuzzy_in_root"),
        ("src/tasks/revive/mod.rs", "pub fn revive_named_in_root"),
        ("src/tasks/skip.rs", "pub fn skip_in_root_with_today"),
        ("src/sync/csv_sync/mod.rs", "pub fn sync_csvs"),
        ("src/sync/counters.rs", "pub fn sync_counters"),
    ] {
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(!source.contains(forbidden), "{path} exposes {forbidden}");
    }
}

#[test]
fn every_task_store_writer_declares_the_shared_owner_boundary() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in [
        "src/tasks/complete/mod.rs",
        "src/tasks/revive/mod.rs",
        "src/tasks/skip.rs",
        "src/tasks/triage_habits/reconcile.rs",
        "src/tasks/schema/mod.rs",
        "src/sync/command/mod.rs",
        "src/command/users/removal.rs",
    ] {
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(
            source.contains("TaskStoreOwner"),
            "task-store writer {path} lacks the shared owner"
        );
    }
    // There is no second family of writers to hold to this any more: the
    // bundled skills ship no executable code, so every task-store writer in
    // existence is in the list above.
    for skill in std::fs::read_dir(root.join("skills")).unwrap() {
        let scripts = skill.unwrap().path().join("scripts");
        assert!(
            !scripts.exists(),
            "{} ships scripts; make them brain subcommands instead",
            scripts.display()
        );
    }
}

#[test]
fn triage_transaction_is_split_into_small_cohesive_modules() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!root.join("src/tasks/triage_habits/transaction.rs").exists());
    for relative in [
        "src/tasks/triage_habits/transaction/mod.rs",
        "src/tasks/triage_habits/transaction/journal.rs",
        "src/tasks/triage_habits/transaction/artifacts.rs",
    ] {
        let source = std::fs::read_to_string(root.join(relative)).unwrap();
        let production_lines = source
            .lines()
            .take_while(|line| *line != "#[cfg(test)]")
            .count();
        assert!(
            production_lines <= 400,
            "{relative} has {production_lines} production lines"
        );
    }
}
