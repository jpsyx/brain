use brain::share::create_archive;
use std::fs;

#[test]
fn archive_recursively_filters_metadata_and_preserves_user_content() {
    let temporary = tempfile::tempdir().unwrap();
    let dir = temporary.path().join("my-dir");
    for folder in [
        "sub/empty",
        ".config",
        ".git",
        "sub/__MACOSX",
        "sub/node_modules",
        ".venv",
    ] {
        fs::create_dir_all(dir.join(folder)).unwrap();
    }
    for file in [
        "paper.pdf",
        "sub/notes.md",
        "sub/data.json",
        ".user-note",
        "sub/.METADATA.json",
        ".METADATA.json",
        "sub/.DS_Store",
        "sub/._paper.pdf",
        "sub/Thumbs.db",
        "sub/desktop.ini",
        ".config/config.json",
        ".git/config",
        "sub/__MACOSX/junk",
        "sub/node_modules/junk",
        ".brain-transaction.json",
        "AGENTS.md",
        "sub/.tasks.csv.brain-triage-1.backup",
        "my-dir.zip",
        "RCLONE_TEST",
        "projects-lookup.csv",
        "sub/zotero-lookup.csv",
        ".tasks_next_id",
        ".habits_next_id",
        ".env",
        "sub/.env.local",
        ".venv/secret",
    ] {
        fs::write(dir.join(file), file).unwrap();
    }
    let archive = create_archive(&dir).unwrap();
    assert_eq!(archive, dir.join("my-dir.zip"));
    let mut zip = zip::ZipArchive::new(fs::File::open(&archive).unwrap()).unwrap();
    let mut names = zip.file_names().map(str::to_owned).collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [
            ".user-note",
            "paper.pdf",
            "sub/",
            "sub/data.json",
            "sub/empty/",
            "sub/notes.md"
        ]
    );
    let mut content = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("paper.pdf").unwrap(), &mut content).unwrap();
    assert_eq!(content, "paper.pdf");
    drop(zip);
    create_archive(&dir).unwrap();
    let zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    assert!(!zip.file_names().any(|name| {
        std::path::Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    }));
    assert!(dir.join(".METADATA.json").exists());
}

#[cfg(unix)]
#[test]
fn archive_skips_symlinks_and_never_overwrites_a_symlink_destination() {
    let temporary = tempfile::tempdir().unwrap();
    let dir = temporary.path().join("share");
    fs::create_dir(&dir).unwrap();
    let private = temporary.path().join("private.txt");
    fs::write(&private, "private").unwrap();
    std::os::unix::fs::symlink(&private, dir.join("linked.txt")).unwrap();
    fs::write(dir.join("public.txt"), "public").unwrap();
    let path = create_archive(&dir).unwrap();
    let zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(zip.file_names().collect::<Vec<_>>(), ["public.txt"]);
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&private, &path).unwrap();
    assert!(create_archive(&dir).is_err());
    assert_eq!(fs::read_to_string(private).unwrap(), "private");
}
