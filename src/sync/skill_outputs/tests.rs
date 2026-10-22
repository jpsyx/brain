use super::*;

const HEADER: &str = "# bisync listing v1 from 2026-10-22T12:00:00.000000000+0000\n";

fn record(path: &str) -> String {
    format!("-        4 - - 2023-11-14T22:13:20.000000000+0000 \"{path}\"\n")
}

#[test]
fn reconciles_active_and_recovery_listings_with_original_backups() {
    let workdir = tempfile::tempdir().unwrap();
    let unchanged = format!(
        "{HEADER}{}{}",
        record("notes/keep.md"),
        record(".config/plugins/custom/SKILL.md")
    );
    let original = format!(
        "{unchanged}{}{}",
        record(".agents/skills/custom/SKILL.md"),
        record(".codex/skills/custom")
    );
    for suffix in [".lst", ".lst-old"] {
        std::fs::write(
            workdir.path().join(format!("pair.path1{suffix}")),
            &original,
        )
        .unwrap();
    }
    std::fs::write(workdir.path().join("unrelated.txt"), &original).unwrap();

    assert_eq!(reconcile_baselines(workdir.path()).unwrap(), 2);
    for suffix in [".lst", ".lst-old"] {
        assert_eq!(
            std::fs::read_to_string(workdir.path().join(format!("pair.path1{suffix}"))).unwrap(),
            unchanged
        );
    }
    assert_eq!(
        std::fs::read_to_string(workdir.path().join("unrelated.txt")).unwrap(),
        original
    );
    let backups: Vec<_> = walkdir::WalkDir::new(workdir.path().join("skill-output-backups"))
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .collect();
    assert_eq!(backups.len(), 2);
    for backup in backups {
        assert_eq!(std::fs::read_to_string(backup.path()).unwrap(), original);
    }
    assert_eq!(reconcile_baselines(workdir.path()).unwrap(), 0);
}

#[test]
fn unknown_listing_format_refuses_before_any_listing_changes() {
    let workdir = tempfile::tempdir().unwrap();
    let original = format!("{HEADER}{}", record(".agents/skills/custom/SKILL.md"));
    let known = workdir.path().join("pair.path1.lst");
    std::fs::write(&known, &original).unwrap();
    std::fs::write(
        workdir.path().join("pair.path2.lst"),
        "# bisync listing v9\n",
    )
    .unwrap();
    assert!(reconcile_baselines(workdir.path()).is_err());
    assert_eq!(std::fs::read_to_string(known).unwrap(), original);
    assert!(!workdir.path().join("skill-output-backups").exists());
}

#[test]
fn quoted_names_and_nested_skill_sources_keep_their_exact_baselines() {
    let workdir = tempfile::tempdir().unwrap();
    let preserved = format!(
        "{HEADER}{}{}{}{}",
        record(r#"notes/a\"quote\\slash\nnewline.md"#),
        record(".config/plugins/custom/.agents/skills/example/SKILL.md"),
        record("projects/example/.agents/skills/custom/SKILL.md"),
        record(".agents/skills-reference.md")
    );
    let original = format!(
        "{preserved}{}",
        record(r#".agents/skills/custom/a\"quote\nnewline.md"#)
    );
    let path = workdir.path().join("pair.path1.lst");
    std::fs::write(&path, original).unwrap();
    reconcile_baselines(workdir.path()).unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), preserved);
}

#[cfg(unix)]
#[test]
fn a_symlinked_listing_refuses_without_touching_its_target() {
    let workdir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let original = format!("{HEADER}{}", record(".agents/skills/custom/SKILL.md"));
    let target = outside.path().join("listing");
    std::fs::write(&target, &original).unwrap();
    std::os::unix::fs::symlink(&target, workdir.path().join("pair.path1.lst")).unwrap();
    assert!(reconcile_baselines(workdir.path()).is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), original);
    assert!(!workdir.path().join("skill-output-backups").exists());
}

#[test]
fn previews_filter_a_disposable_copy_without_changing_live_baselines() {
    let workdir = tempfile::tempdir().unwrap();
    let original = format!(
        "{HEADER}{}{}",
        record("note.md"),
        record(".agents/skills/custom/SKILL.md")
    );
    let path = workdir.path().join("pair.path1.lst");
    std::fs::write(&path, &original).unwrap();
    let preview = preview_baselines(workdir.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(preview.path().join("pair.path1.lst")).unwrap(),
        format!("{HEADER}{}", record("note.md"))
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let temporary = preview.path().to_path_buf();
    drop(preview);
    assert!(!temporary.exists());
    assert_eq!(std::fs::read_dir(workdir.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn a_symlinked_backup_directory_refuses_before_any_replacement() {
    let workdir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let original = format!("{HEADER}{}", record(".agents/skills/custom/SKILL.md"));
    let listing = workdir.path().join("pair.path1.lst");
    std::fs::write(&listing, &original).unwrap();
    std::os::unix::fs::symlink(outside.path(), workdir.path().join("skill-output-backups"))
        .unwrap();
    assert!(reconcile_baselines(workdir.path()).is_err());
    assert_eq!(std::fs::read_to_string(listing).unwrap(), original);
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn incomplete_unused_scratch_does_not_block_a_valid_baseline() {
    let workdir = tempfile::tempdir().unwrap();
    let original = format!("{HEADER}{}", record(".agents/skills/custom/SKILL.md"));
    std::fs::write(workdir.path().join("pair.path1.lst"), original).unwrap();
    for suffix in [".lst-new", ".lst-err"] {
        std::fs::write(
            workdir.path().join(format!("pair.path1{suffix}")),
            "interrupted write",
        )
        .unwrap();
    }
    assert_eq!(reconcile_baselines(workdir.path()).unwrap(), 1);
    for suffix in [".lst-new", ".lst-err"] {
        assert_eq!(
            std::fs::read_to_string(workdir.path().join(format!("pair.path1{suffix}"))).unwrap(),
            "interrupted write"
        );
    }
}
