use super::*;

#[cfg(unix)]
#[test]
fn preexisting_backup_base_symlink_into_workspace_is_rejected() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    fs::create_dir_all(&root).unwrap();
    let base = temporary.path().join("cache-link");
    symlink(&root, &base).unwrap();
    let backup = base.join("20260806T120000Z-pre-multi-workspace");

    let error = backup_portable_data(&root, &base, &backup).unwrap_err();

    assert!(error.to_string().contains("disjoint"), "{error:#}");
    assert!(!root.join("20260806T120000Z-pre-multi-workspace").exists());
}

#[cfg(unix)]
#[test]
fn preexisting_nested_backup_symlink_into_workspace_is_rejected() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let tasks = root.join("tasks");
    let base = temporary.path().join("cache/migration-backups");
    let backup = base.join("20260806T120000Z-pre-multi-workspace");
    fs::create_dir_all(&tasks).unwrap();
    fs::create_dir_all(&backup).unwrap();
    for (name, bytes) in [
        ("tasks.csv", b"task_id\nT1\n".as_slice()),
        ("habits.csv", b"task_id\nH1\n".as_slice()),
        ("SCHEMA.json", b"{}\n".as_slice()),
    ] {
        fs::write(tasks.join(name), bytes).unwrap();
    }
    symlink(&tasks, backup.join("tasks")).unwrap();

    let error = backup_portable_data(&root, &base, &backup).unwrap_err();

    assert!(error.to_string().contains("symlink"), "{error:#}");
    assert_eq!(fs::read(tasks.join("tasks.csv")).unwrap(), b"task_id\nT1\n");
}

#[test]
fn preexisting_nested_backup_file_component_is_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let tasks = root.join("tasks");
    let base = temporary.path().join("cache/migration-backups");
    let backup = base.join("20260806T120000Z-pre-multi-workspace");
    fs::create_dir_all(&tasks).unwrap();
    fs::create_dir_all(&backup).unwrap();
    for (name, bytes) in [
        ("tasks.csv", b"task_id\nT1\n".as_slice()),
        ("habits.csv", b"task_id\nH1\n".as_slice()),
        ("SCHEMA.json", b"{}\n".as_slice()),
    ] {
        fs::write(tasks.join(name), bytes).unwrap();
    }
    fs::write(backup.join("tasks"), b"not a directory\n").unwrap();

    let error = backup_portable_data(&root, &base, &backup).unwrap_err();

    assert!(error.to_string().contains("not a directory"), "{error:#}");
    assert_eq!(fs::read(tasks.join("tasks.csv")).unwrap(), b"task_id\nT1\n");
}

#[cfg(unix)]
#[test]
fn backup_publish_rejects_parent_replacement_after_validation() {
    use std::cell::Cell;
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let root_config = root.join(".config");
    let root_tasks = root.join("tasks");
    let base = temporary.path().join("cache/migration-backups");
    let backup = base.join("20260806T120000Z-pre-multi-workspace");
    fs::create_dir_all(&root_config).unwrap();
    fs::create_dir_all(&root_tasks).unwrap();
    fs::create_dir_all(&backup).unwrap();
    fs::write(root_config.join("config.json"), b"portable-config\n").unwrap();
    let escaped_temp = Cell::new(false);

    let error = backup_portable_data_with_hook(&root, &base, &backup, |relative, step| {
        if relative == Path::new(".config/config.json") && step == BackupWriteStep::BeforePublish {
            fs::remove_dir_all(backup.join(".config"))?;
            symlink(&root_tasks, backup.join(".config"))?;
            escaped_temp.set(fs::read_dir(&root_tasks)?.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".config.json.")
            }));
            return Err(std::io::Error::other("stop after destination observation"));
        }
        Ok(())
    })
    .unwrap_err();

    assert!(
        format!("{error:#}").contains("stop after destination observation"),
        "{error:#}"
    );
    assert!(!escaped_temp.get());
}

#[test]
fn backup_publish_failure_leaves_live_data_and_failed_destination_unchanged() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let base = temporary.path().join("cache/migration-backups");
    let backup = base.join("20260806T120000Z-pre-multi-workspace");
    let files = [
        (
            "tasks/tasks.csv",
            b"task_id,task_name\nT1,Plan\n".as_slice(),
        ),
        (
            "tasks/habits.csv",
            b"task_id,task_name\nH1,Walk\n".as_slice(),
        ),
        ("tasks/SCHEMA.json", b"{}\n".as_slice()),
    ];
    for (relative, bytes) in files {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    let error = backup_portable_data_with_hook(&root, &base, &backup, |relative, step| {
        if relative == Path::new("tasks/habits.csv") && step == BackupWriteStep::BeforePublish {
            return Err(std::io::Error::other("injected backup publish failure"));
        }
        Ok(())
    })
    .unwrap_err();

    assert!(format!("{error:#}").contains("injected backup publish failure"));
    for (relative, bytes) in files {
        assert_eq!(fs::read(root.join(relative)).unwrap(), bytes);
    }
    assert!(!backup.join("tasks/habits.csv").exists());
    assert!(
        fs::read_dir(backup.join("tasks"))
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().contains("habits.csv"))
    );
}
