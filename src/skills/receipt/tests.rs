use super::*;
use crate::skills::install_transaction::InstallVersion;

#[test]
fn receipt_peek_reads_committed_wal_without_source_writes_or_schema_creation() {
    const CHILD: &str = "BRAIN_TEST_RECEIPT_WAL_CHILD";
    let receipt = InstallReceipt {
        version: InstallVersion::current(),
        manifest_sha256: "a".repeat(64),
    };
    if let Some(path) = std::env::var_os(CHILD) {
        let connection = rusqlite::Connection::open(path).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT); PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        connection
            .execute(
                "INSERT INTO meta VALUES ('skills_install_receipt',?1)",
                [serde_json::to_string(&receipt).unwrap()],
            )
            .unwrap();
        std::process::exit(0);
    }
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state.db");
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", "skills::receipt::tests::receipt_peek_reads_committed_wal_without_source_writes_or_schema_creation"]).env(CHILD, &path).output().unwrap();
    assert!(output.status.success());
    std::fs::remove_file(path.with_extension("db-shm")).unwrap();
    let before = std::fs::read(&path).unwrap();
    let wal = std::fs::read(path.with_extension("db-wal")).unwrap();
    let stamp = WorkspaceStamp {
        path: path.clone(),
        workspace_id: "fixture".into(),
        actor_id: "member".into(),
    };
    assert_eq!(stamp.peek().unwrap(), Some(receipt));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read(path.with_extension("db-wal")).unwrap(), wal);
    assert!(!path.with_extension("db-shm").exists());
    let missing = WorkspaceStamp {
        path: temporary.path().join("absent.db"),
        workspace_id: "fixture".into(),
        actor_id: "member".into(),
    };
    assert_eq!(missing.peek().unwrap(), None);
    assert!(!missing.path.exists());
}
