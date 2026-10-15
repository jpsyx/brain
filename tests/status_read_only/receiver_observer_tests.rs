use super::*;
use crate::snapshot::snapshot;

const ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

fn fixture() -> (tempfile::TempDir, PathBuf, brain::workspace::WorkspaceId) {
    let home = tempfile::tempdir().unwrap();
    let id = brain::workspace::WorkspaceId::parse(ID).unwrap();
    let db = brain::workspace::WorkspacePaths::new(home.path(), id).state_db();
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    std::fs::write(&db, b"canonical database").unwrap();
    (home, db, id)
}

#[test]
fn allows_only_matching_regular_coordination_creation_and_shm_refresh() {
    let (home, db, id) = fixture();
    let before = snapshot(home.path());
    std::fs::write(db.with_file_name("state.db-shm"), b"coordination").unwrap();
    std::fs::write(db.with_file_name("state.db-wal"), b"").unwrap();
    let coordinated = snapshot(home.path());
    assert!(compare(&before, &coordinated, &[id]).is_ok());
    std::fs::write(db.with_file_name("state.db-shm"), b"refreshed coordination").unwrap();
    assert!(compare(&coordinated, &snapshot(home.path()), &[id]).is_ok());
}

#[test]
fn rejects_canonical_off_uuid_and_nonempty_new_wal_changes() {
    for mutation in ["canonical", "off-uuid", "nonempty-wal", "new-database"] {
        let (home, db, id) = fixture();
        let before = snapshot(home.path());
        match mutation {
            "canonical" => std::fs::write(&db, b"changed domain data").unwrap(),
            "off-uuid" => {
                let other = db.parent().unwrap().parent().unwrap().join("unselected");
                std::fs::create_dir(&other).unwrap();
                std::fs::write(other.join("state.db-shm"), b"coordination").unwrap();
            }
            "nonempty-wal" => std::fs::write(db.with_file_name("state.db-wal"), b"commit").unwrap(),
            "new-database" => std::fs::write(db.with_file_name("another.db"), b"new store").unwrap(),
            _ => unreachable!(),
        }
        assert!(compare(&before, &snapshot(home.path()), &[id]).is_err(), "{mutation}");
    }
}

#[test]
fn preserves_existing_wal_bytes_and_requires_a_baseline_database() {
    let (home, db, id) = fixture();
    let wal = db.with_file_name("state.db-wal");
    std::fs::write(&wal, b"committed WAL").unwrap();
    let before = snapshot(home.path());
    std::fs::write(&wal, b"different WAL").unwrap();
    assert!(compare(&before, &snapshot(home.path()), &[id]).is_err());
    std::fs::remove_file(&db).unwrap();
    let without_db = snapshot(home.path());
    std::fs::write(db.with_file_name("state.db-shm"), b"coordination").unwrap();
    assert!(compare(&without_db, &snapshot(home.path()), &[id]).is_err());
}

#[test]
fn rejects_coordination_type_symlink_and_mode_replacement() {
    use std::os::unix::fs::PermissionsExt as _;
    for mutation in ["directory", "symlink", "mode"] {
        let (home, db, id) = fixture();
        let shm = db.with_file_name("state.db-shm");
        std::fs::write(&shm, b"coordination").unwrap();
        let before = snapshot(home.path());
        match mutation {
            "directory" => {
                std::fs::remove_file(&shm).unwrap();
                std::fs::create_dir(&shm).unwrap();
            }
            "symlink" => {
                std::fs::remove_file(&shm).unwrap();
                std::os::unix::fs::symlink(&db, &shm).unwrap();
            }
            "mode" => std::fs::set_permissions(&shm, std::fs::Permissions::from_mode(0o700)).unwrap(),
            _ => unreachable!(),
        }
        assert!(compare(&before, &snapshot(home.path()), &[id]).is_err(), "{mutation}");
    }
}

#[test]
fn rejects_coordination_refresh_for_an_existing_unselected_uuid() {
    let (home, db, id) = fixture();
    let other_id = brain::workspace::WorkspaceId::new();
    let other_db = brain::workspace::WorkspacePaths::new(home.path(), other_id).state_db();
    std::fs::create_dir_all(other_db.parent().unwrap()).unwrap();
    std::fs::write(&other_db, b"other canonical database").unwrap();
    std::fs::write(other_db.with_file_name("state.db-shm"), b"before").unwrap();
    let before = snapshot(home.path());
    std::fs::write(other_db.with_file_name("state.db-shm"), b"after").unwrap();
    assert!(compare(&before, &snapshot(home.path()), &[id]).is_err());
    assert!(db.is_file());
}
