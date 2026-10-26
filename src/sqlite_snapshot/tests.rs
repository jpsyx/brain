use super::*;
use std::{cell::Cell, os::unix::fs::PermissionsExt as _};

#[test]
fn changed_capture_is_bounded_and_never_queried() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state.db");
    fs::write(&path, "initial").unwrap();
    let attempts = Cell::new(0);
    assert!(
        read_with(
            &path,
            |_| -> Result<()> { panic!("unstable image reached SQLite") },
            || {
                attempts.set(attempts.get() + 1);
                fs::write(&path, attempts.get().to_string()).unwrap();
            }
        )
        .is_err()
    );
    assert_eq!(attempts.get(), ATTEMPTS);
}

#[test]
fn copied_database_is_private_and_removed_after_query() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("CREATE TABLE evidence(value TEXT); INSERT INTO evidence VALUES('fixture');")
        .unwrap();
    drop(connection);
    let private = read(&path, |connection| {
        let private = PathBuf::from(connection.path().unwrap());
        assert_eq!(fs::metadata(&private)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(private.parent().unwrap())?
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let value: String =
            connection.query_row("SELECT value FROM evidence", [], |row| row.get(0))?;
        assert_eq!(value, "fixture");
        Ok(private)
    })
    .unwrap()
    .unwrap();
    assert!(!private.parent().unwrap().exists());
    fs::OpenOptions::new()
        .write(true)
        .open(path.clone())
        .unwrap()
        .set_len(LIMIT + 1)
        .unwrap();
    assert!(read(&path, |_| Ok(())).is_err());
}
