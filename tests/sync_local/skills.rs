use super::*;

const SKILL_DIRECTORIES: [&str; 4] = [
    ".agents/skills",
    ".claude/skills",
    ".codex/skills",
    ".opencode/skills",
];

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn cloud_sync_preserves_local_skill_builds_and_transfers_their_sources() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let local = base.path().join("local");
    let remote = base.path().join("remote");
    for directory in SKILL_DIRECTORIES {
        write(
            &local,
            &format!("{directory}/local/SKILL.md"),
            "local build",
        );
        write(
            &remote,
            &format!("{directory}/remote/SKILL.md"),
            "old remote build",
        );
    }
    write(&local, ".config/extensions/todo.md", "local extension");
    write(
        &local,
        ".config/plugins/custom/.agents/skills/example/SKILL.md",
        "plugin asset",
    );
    write(
        &remote,
        ".config/plugins/custom/SKILL.md",
        "remote plugin source",
    );
    write(&remote, ".opencode/plugins/brain.js", "frontend bridge");

    for direction in [Direction::Resync, Direction::Pull, Direction::Both] {
        let outcome = run(&local, &remote, direction);
        assert!(outcome.exit_ok, "sync failed: {outcome:?}");
        for directory in SKILL_DIRECTORIES {
            assert!(
                !local.join(format!("{directory}/remote/SKILL.md")).exists(),
                "downloaded generated skill in {directory}"
            );
            assert!(
                !remote.join(format!("{directory}/local/SKILL.md")).exists(),
                "uploaded generated skill in {directory}"
            );
            assert_eq!(
                std::fs::read_to_string(local.join(format!("{directory}/local/SKILL.md"))).unwrap(),
                "local build"
            );
            assert_eq!(
                std::fs::read_to_string(remote.join(format!("{directory}/remote/SKILL.md")))
                    .unwrap(),
                "old remote build"
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(remote.join(".config/extensions/todo.md")).unwrap(),
        "local extension"
    );
    assert_eq!(
        std::fs::read_to_string(
            remote.join(".config/plugins/custom/.agents/skills/example/SKILL.md")
        )
        .unwrap(),
        "plugin asset"
    );
    assert_eq!(
        std::fs::read_to_string(local.join(".config/plugins/custom/SKILL.md")).unwrap(),
        "remote plugin source"
    );
    assert_eq!(
        std::fs::read_to_string(local.join(".opencode/plugins/brain.js")).unwrap(),
        "frontend bridge"
    );
}

#[test]
fn automatic_upload_leaves_generated_skills_on_their_own_machine() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let local = base.path().join("local");
    let remote = base.path().join("remote");
    for directory in SKILL_DIRECTORIES {
        write(&local, &format!("{directory}/custom/SKILL.md"), "generated");
    }
    write(&local, ".config/plugins/custom/SKILL.md", "plugin source");
    write(&local, ".config/extensions/todo.md", "extension source");
    let argv =
        brain::sync::args::push_args(&cfg(), &local.to_string_lossy(), &remote.to_string_lossy());
    let (ok, output) = brain::sync::run::run_rclone_capture(&[], &argv);
    assert!(ok, "upload failed: {output}");
    for directory in SKILL_DIRECTORIES {
        assert!(!remote.join(directory).exists(), "uploaded {directory}");
    }
    assert_eq!(
        std::fs::read_to_string(remote.join(".config/plugins/custom/SKILL.md")).unwrap(),
        "plugin source"
    );
    assert_eq!(
        std::fs::read_to_string(remote.join(".config/extensions/todo.md")).unwrap(),
        "extension source"
    );
}

#[test]
fn legacy_skill_baselines_preserve_note_deletions_and_keep_both_conflicts() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let local = base.path().join("local");
    let remote = base.path().join("remote");
    let paths = workspace_paths(base.path(), workspace_id());
    let workdir = brain::sync::run::bisync_workdir(&paths);
    std::fs::create_dir_all(&workdir).unwrap();
    for root in [&local, &remote] {
        for directory in SKILL_DIRECTORIES {
            for index in 0..4 {
                write(root, &format!("{directory}/demo/{index}.md"), "old build");
            }
        }
        for name in ["RCLONE_TEST", "keep.md", "delete.md", "conflict.md"] {
            write(root, name, "baseline");
        }
        write(root, ".config/extensions/todo.md", "extension");
        write(root, ".config/plugins/custom/SKILL.md", "plugin");
        for entry in walkdir::WalkDir::new(root).into_iter().map(Result::unwrap) {
            if entry.file_type().is_file() {
                set_time(entry.path(), 0);
            }
        }
    }
    let mut config = cfg();
    config.max_delete_percent = 50;
    let argv = bisync_args(
        &config,
        &local.to_string_lossy(),
        &remote.to_string_lossy(),
        &workdir.to_string_lossy(),
        Direction::Resync,
    );
    let mut legacy = Vec::new();
    let mut arguments = argv.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--exclude" {
            let pattern = arguments.next().unwrap();
            if !pattern.ends_with("/skills/**") {
                legacy.extend([argument, pattern]);
            }
        } else {
            legacy.push(argument);
        }
    }
    let (ok, output) = brain::sync::run::run_rclone_capture(&[], &legacy);
    assert!(ok, "legacy baseline failed: {output}");
    std::fs::remove_file(local.join("delete.md")).unwrap();
    write(&local, "conflict.md", "local conflicting edit");
    set_time(&local.join("conflict.md"), 10);
    write(&remote, "conflict.md", "remote conflicting edit is newer");
    set_time(&remote.join("conflict.md"), 20);
    write(
        &remote,
        ".agents/skills/demo/0.md",
        "different remote generated content",
    );
    write(
        &remote,
        ".agents/skills/remote-only/SKILL.md",
        "stale build",
    );
    write(
        &local,
        ".config/extensions/todo.md",
        "updated local extension",
    );
    set_time(&local.join(".config/extensions/todo.md"), 10);

    let preview = brain::sync::skill_outputs::preview_baselines(&workdir).unwrap();
    let mut preview_args = bisync_args(
        &config,
        &local.to_string_lossy(),
        &remote.to_string_lossy(),
        &preview.path().to_string_lossy(),
        Direction::Both,
    );
    preview_args.extend([
        "--compare".to_owned(),
        "size,checksum".to_owned(),
        "--dry-run".to_owned(),
    ]);
    let (ok, output) = brain::sync::run::run_rclone_capture(&[], &preview_args);
    assert!(ok, "preview of legacy baseline failed: {output}");
    assert!(
        remote.join("delete.md").exists(),
        "preview changed live files"
    );
    drop(preview);

    brain::sync::skill_outputs::reconcile_baselines(&workdir).unwrap();
    let argv = bisync_args(
        &config,
        &local.to_string_lossy(),
        &remote.to_string_lossy(),
        &workdir.to_string_lossy(),
        Direction::Both,
    );
    let (ok, output) = brain::sync::run::run_rclone_capture(&[], &argv);
    assert!(ok, "upgraded sync failed: {output}");
    assert!(
        !remote.join("delete.md").exists(),
        "a baseline reset resurrected a deleted note"
    );
    for root in [&local, &remote] {
        assert_eq!(
            std::fs::read_to_string(root.join("keep.md")).unwrap(),
            "baseline"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("conflict.md")).unwrap(),
            "remote conflicting edit is newer"
        );
    }
    let conflict_copies: Vec<_> = walkdir::WalkDir::new(&local)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("__brainconflict__")
        })
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
        .collect();
    assert!(
        conflict_copies
            .iter()
            .any(|content| content == "local conflicting edit"),
        "losing note edit must survive"
    );
    assert_eq!(
        std::fs::read_to_string(local.join(".agents/skills/demo/0.md")).unwrap(),
        "old build"
    );
    assert_eq!(
        std::fs::read_to_string(remote.join(".agents/skills/demo/0.md")).unwrap(),
        "different remote generated content"
    );
    assert!(!local.join(".agents/skills/remote-only/SKILL.md").exists());
    assert_eq!(
        std::fs::read_to_string(remote.join(".config/extensions/todo.md")).unwrap(),
        "updated local extension"
    );
}

fn set_time(path: &Path, offset: u64) {
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + offset);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}
