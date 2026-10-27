use super::*;

#[test]
fn explicit_repair_retries_a_pending_receive_once_and_preserves_failed_evidence() {
    let transfer = super::sync_fixture::Transfer::new();
    let _ = transfer.pull("fail");
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    assert!(
        !transfer
            .run(&["sync", "repair", "-w", "alpha"], "fail")
            .status
            .success()
    );
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    assert_eq!(
        transfer.transfers(),
        2,
        "repair must reach one verified transfer"
    );
    let recovered = transfer.run(&["sync", "repair", "-w", "alpha"], "complete");
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(
        transfer.transfers(),
        3,
        "recovery must consume the invocation"
    );
    assert_eq!(transfer.phase(), None);
    transfer.fixture.startup();
}

#[test]
fn a_crashed_receive_restarts_with_one_verified_pull_before_reconciliation() {
    let transfer = super::sync_fixture::Transfer::new();
    let crashed = transfer.pull("crash");
    assert_eq!(
        crashed.status.code(),
        None,
        "{}",
        String::from_utf8_lossy(&crashed.stderr)
    );
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    assert!(
        !transfer
            .fixture
            .root
            .join(".config/extensions/workspace.md")
            .exists()
    );
    let blocked = transfer.fixture.run(&["server", "status"]);
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("sync --pull"));
    let failed = transfer.pull("fail");
    assert!(!failed.status.success());
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    let recovered = transfer.pull("complete");
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(
        transfer.transfers(),
        3,
        "each invocation must complete at most one pull"
    );
    assert_eq!(transfer.phase(), None);
    transfer.fixture.startup();
    assert!(
        std::fs::read_to_string(transfer.fixture.root.join("AGENTS.md"))
            .unwrap()
            .contains("Arrived together.")
    );
}

#[test]
fn received_generated_text_is_replaced_from_available_extension_sources() {
    let transfer = super::sync_fixture::Transfer::new();
    let first = transfer.pull("mismatch");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(transfer.phase(), None);
    let path = transfer.fixture.root.join("AGENTS.md");
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("Arrived together.")
    );
    let before = transfer.transfers();
    let next = transfer.pull("complete");
    assert!(
        next.status.success(),
        "{}",
        String::from_utf8_lossy(&next.stderr)
    );
    assert_eq!(transfer.transfers(), before + 1);
    assert_eq!(transfer.phase(), None);
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("Arrived together.")
    );
}

pub(super) fn receiving(fixture: &Fixture, phase: &str) {
    let path = fixture.cache().join("sync/journal.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch("CREATE TABLE IF NOT EXISTS portable_receive (slot INTEGER PRIMARY KEY CHECK(slot = 1), workspace_id TEXT NOT NULL, phase TEXT NOT NULL, started_at TEXT NOT NULL);").unwrap();
    db.execute(
        "INSERT OR REPLACE INTO portable_receive VALUES (1, ?1, ?2, '2026-10-25T00:00:00Z')",
        [ID, phase],
    )
    .unwrap();
}

#[test]
fn interrupted_receive_blocks_even_when_the_previous_package_is_intact() {
    let fixture = Fixture::new("0.105.3");
    fixture.startup();
    let before = std::fs::read(fixture.root.join("AGENTS.md")).unwrap();
    receiving(&fixture, "receiving");
    for args in [
        &["server", "status"][..],
        &["sync", "--push", "-w", "alpha"][..],
        &["skills", "sync", "--all-workspaces", "--json"][..],
    ] {
        let output = fixture.run(args);
        assert!(
            !output.status.success(),
            "pending receive reached dispatch: {args:?}"
        );
        assert_eq!(
            std::fs::read(fixture.root.join("AGENTS.md")).unwrap(),
            before
        );
    }
}

#[test]
fn a_completed_receive_applies_changed_sources_even_with_optional_sync_disabled() {
    let fixture = Fixture::new("0.105.3");
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"skills_auto_sync": false}),
    );
    fixture.startup();
    let extensions = fixture.root.join(".config/extensions");
    std::fs::create_dir_all(&extensions).unwrap();
    std::fs::write(
        extensions.join("workspace.md"),
        "[workspace:context]\nReceived shared context.\n",
    )
    .unwrap();
    receiving(&fixture, "reconcile");
    fixture.startup();
    assert!(
        std::fs::read_to_string(fixture.root.join("AGENTS.md"))
            .unwrap()
            .contains("Received shared context.")
    );
    let db = rusqlite::Connection::open(fixture.cache().join("sync/journal.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM portable_receive", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn completed_supported_arrivals_adopt_missing_or_stale_receipts_in_either_order() {
    for source_first in [false, true] {
        for missing_receipt in [false, true] {
            let transfer = super::sync_fixture::Transfer::new();
            let fixture = &transfer.fixture;
            write_json(
                &fixture.root.join(".config/config.json"),
                &json!({"allowed_skills":["todo"], "skills_auto_sync":false}),
            );
            let manifest = fixture.cache().join("skills-install/managed.json");
            if missing_receipt {
                std::fs::remove_file(manifest).unwrap();
            } else {
                let mut value = read_json(&manifest);
                value["binary_version"] = json!("0.105.3");
                write_json(&manifest, &value);
            }
            receiving(fixture, "receiving");
            let source = ".config/extensions/workspace.md";
            std::fs::create_dir_all(fixture.root.join(".config/extensions")).unwrap();
            let (first, second) = if source_first {
                (source, "AGENTS.md")
            } else {
                ("AGENTS.md", source)
            };
            std::fs::copy(transfer.remote.join(first), fixture.root.join(first)).unwrap();
            assert!(!fixture.run(&["server", "status"]).status.success());
            std::fs::copy(transfer.remote.join(second), fixture.root.join(second)).unwrap();
            receiving(fixture, "reconcile");
            fixture.startup();
            assert_eq!(transfer.phase(), None);
            assert_eq!(
                read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
                json!(["todo"])
            );
            assert!(
                std::fs::read_to_string(fixture.root.join("AGENTS.md"))
                    .unwrap()
                    .contains("Arrived together.")
            );
        }
    }
}

#[test]
fn generated_document_edits_are_repaired_during_receive_reconciliation() {
    for pending in [false, true] {
        let transfer = super::sync_fixture::Transfer::new();
        let path = transfer.fixture.root.join("AGENTS.md");
        let bytes = std::fs::read_to_string(&path).unwrap();
        let changed = if pending {
            bytes.replace("schema=1", "schema=99")
        } else {
            bytes.replace("# Brain workspace", "# Edited workspace")
        };
        assert_ne!(changed, bytes);
        std::fs::write(&path, &changed).unwrap();
        if pending {
            receiving(&transfer.fixture, "reconcile");
        }
        let output = transfer.pull("complete");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(transfer.transfers(), 1);
        assert_eq!(transfer.phase(), None);
        let repaired = std::fs::read_to_string(path).unwrap();
        assert!(!repaired.contains("schema=99"));
        assert!(!repaired.contains("# Edited workspace"));
        assert!(repaired.contains("Arrived together."));
    }
}

#[test]
fn interrupted_pull_runs_earlier_prerequisites_before_any_recovery_transport() {
    let transfer = super::sync_fixture::Transfer::new();
    assert_eq!(transfer.pull("crash").status.code(), None);
    let hooks = transfer.fixture.root.join(".brain");
    if hooks.is_dir() {
        std::fs::remove_dir_all(&hooks).unwrap();
    }
    std::fs::write(&hooks, "prerequisite obstruction").unwrap();
    std::fs::write(
        transfer
            .fixture
            .home
            .join(".config/brain/migrations/version"),
        "0.70.0",
    )
    .unwrap();
    let failed = transfer.pull("complete");
    assert!(!failed.status.success());
    assert_eq!(transfer.transfers(), 1);
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    std::fs::remove_file(hooks).unwrap();
    let recovered = transfer.pull("complete");
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(transfer.transfers(), 2);
    assert_eq!(transfer.phase(), None);
}
