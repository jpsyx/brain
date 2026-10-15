//! A compatibility receipt may create state without inventing receiver work.
use std::path::Path;

pub(super) fn assert_empty(path: &Path) {
    let connection = rusqlite::Connection::open(path).unwrap();
    let receipt: String = connection
        .query_row(
            "SELECT value FROM meta WHERE key='skills_install_receipt'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let receipt: serde_json::Value = serde_json::from_str(&receipt).unwrap();
    assert_eq!(
        receipt["version"]["binary_version"],
        env!("CARGO_PKG_VERSION")
    );
    for table in [
        "brain_sessions",
        "manual_sessions",
        "receiver_jobs",
        "receiver_conversations",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "compatibility created work in {table}");
    }
}
