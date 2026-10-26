use super::*;
use std::{collections::BTreeMap, fs};

const ID: &str = "11111111-1111-4111-8111-111111111111";

#[test]
fn wal_only_pending_receive_is_visible_without_creating_source_sidecars() {
    const CHILD: &str = "BRAIN_TEST_RECEIVE_WAL_CHILD";
    if let Some(path) = std::env::var_os(CHILD) {
        let journal = Journal::open(Path::new(&path)).unwrap();
        journal.begin_receive(ID, "fixture").unwrap();
        std::process::exit(0);
    }
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("journal.db");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sync::journal::receive::tests::wal_only_pending_receive_is_visible_without_creating_source_sidecars"])
        .env(CHILD, &path)
        .output()
        .unwrap();
    assert!(output.status.success());
    fs::remove_file(path.with_extension("db-shm")).unwrap();
    let snapshot = || {
        use sha2::Digest as _;
        fs::read_dir(temporary.path())
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name(),
                    format!(
                        "{:x}",
                        sha2::Sha256::digest(fs::read(entry.path()).unwrap())
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let before = snapshot();
    assert_eq!(
        Journal::receive_phase(&path, ID).unwrap(),
        Some(ReceivePhase::Receiving)
    );
    assert_eq!(
        snapshot(),
        before,
        "read-only probe changed original journal files"
    );
}

#[test]
fn receive_probe_rejects_a_symlinked_database() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("journal.db");
    Journal::open(&path)
        .unwrap()
        .begin_receive(ID, "fixture")
        .unwrap();
    let link = temporary.path().join("linked.db");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(Journal::receive_phase(&link, ID).is_err());
}
