use super::*;
use sha2::{Digest as _, Sha256};
use std::{fs, io::Read as _};

fn fingerprint(path: &Path) -> (String, std::time::SystemTime) {
    let mut reader =
        fs::File::open(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut digest = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = reader.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    (
        format!("{:x}", digest.finalize()),
        fs::metadata(path).unwrap().modified().unwrap(),
    )
}

#[test]
fn currentness_accepts_large_valid_database_and_wal_without_source_writes() {
    const CHILD: &str = "BRAIN_TEST_LARGE_SQLITE";
    if let Some(path) = std::env::var_os(CHILD) {
        let connection = rusqlite::Connection::open(path).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE unrelated(payload BLOB); PRAGMA wal_checkpoint(TRUNCATE); INSERT INTO unrelated VALUES(zeroblob(67108865));").unwrap();
        if std::env::var("BRAIN_TEST_LARGE_WAL").unwrap() == "false" {
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
        }
        std::process::exit(0);
    }
    for wal in [false, true] {
        let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
        fixture.startup();
        let path = fixture.cache().join("state.db");
        let seeded = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "release_checks::currentness_accepts_large_valid_database_and_wal_without_source_writes"])
            .env(CHILD, &path).env("BRAIN_TEST_LARGE_WAL", wal.to_string()).output().unwrap();
        assert!(
            seeded.status.success(),
            "{}",
            String::from_utf8_lossy(&seeded.stderr)
        );
        let wal_path = path.with_extension("db-wal");
        assert!(
            fs::metadata(if wal { &wal_path } else { &path })
                .unwrap()
                .len()
                > 64 * 1024 * 1024
        );
        let before = [fingerprint(&path), fingerprint(&wal_path)];
        let managed = fixture.cache().join("skills-install/managed.json");
        let contract = fixture.root.join("AGENTS.md");
        let package = [fingerprint(&managed), fingerprint(&contract)];
        // Patch startup isolates compatibility observation from older schema callbacks.
        fs::write(
            fixture.home.join(".config/brain/migrations/version"),
            "0.106.14",
        )
        .unwrap();
        fixture.startup();
        assert_eq!([fingerprint(&path), fingerprint(&wal_path)], before);
        assert_eq!([fingerprint(&managed), fingerprint(&contract)], package);
        let connection = rusqlite::Connection::open(&path).unwrap();
        let integrity: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        drop(connection);
        // Same-version startup also runs historical schema callbacks, which may
        // checkpoint WAL. Availability and logical contents remain unchanged.
        fixture.startup();
        let connection = rusqlite::Connection::open(&path).unwrap();
        let length: u64 = connection
            .query_row("SELECT length(payload) FROM unrelated", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(length, 67_108_865);
        assert_eq!([fingerprint(&managed), fingerprint(&contract)], package);
    }
}

#[test]
fn invalid_machine_skill_repairs_refuse_without_writes_or_sensitive_diagnostics() {
    let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
    fixture.startup();
    assert!(
        fixture
            .run(&["env", "set", "agent_capabilities.skills=[]", "-w", "alpha"])
            .status
            .success()
    );
    let path = fixture.home.join(".config/brain/env.json");
    let before = fs::read(&path).unwrap();
    for assignment in [
        "agent_capabilities.skills=SECRET_DECLARATION",
        "agent_capabilities={\"skills\":[{\"name\":\"agenda\",\"path\":\"/SECRET_PATH\"}]}",
    ] {
        let result = fixture.run(&["env", "set", assignment, "-w", "alpha"]);
        assert!(!result.status.success());
        assert_eq!(fs::read(&path).unwrap(), before);
        let public = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!public.contains("SECRET"), "{public}");
    }
}

#[test]
fn skill_repair_keeps_identity_scope_and_reconciles_after_committed_correction() {
    let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
    fixture.startup();
    assert!(
        fixture
            .run(&["env", "set", "agent_capabilities.skills=[]", "-w", "alpha"])
            .status
            .success()
    );
    let path = fixture.home.join(".config/brain/env.json");
    let mut registry = read_json(&path);
    registry["workspaces"]["alpha"]["env"]["agent_capabilities"] = json!("broken");
    write_json(&path, &registry);
    let before = fs::read(&path).unwrap();
    let unrelated = fixture.run(&["env", "set", "claude_cmd=synthetic-command", "-w", "alpha"]);
    assert!(!unrelated.status.success());
    assert_eq!(fs::read(&path).unwrap(), before);
    let mismatch = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "env",
            "set",
            "agent_capabilities={\"skills\":[]}",
            "-w",
            "alpha",
        ])
        .env("HOME", &fixture.home)
        .env("XDG_CONFIG_HOME", fixture.home.join(".config"))
        .env("BRAIN_WORKSPACE_ID", "22222222-2222-4222-8222-222222222222")
        .env_remove("BRAIN_WORKSPACE")
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("does not match"));
    assert_eq!(fs::read(&path).unwrap(), before);
    let skill = fixture.root.join(".agents/skills/agenda/SKILL.md");
    let original = fs::read(&skill).unwrap();
    fs::write(&skill, "user modification").unwrap();
    let committed = fixture.run(&[
        "env",
        "set",
        "agent_capabilities={\"skills\":[]}",
        "-w",
        "alpha",
    ]);
    assert!(!committed.status.success());
    let error = String::from_utf8_lossy(&committed.stderr);
    assert!(
        error.contains("repair committed; mandatory startup did not complete"),
        "{error}"
    );
    assert_eq!(
        read_json(&path)["workspaces"]["alpha"]["env"]["agent_capabilities"],
        json!({"skills":[]})
    );
    assert_eq!(fs::read_to_string(&skill).unwrap(), "user modification");
    fs::write(&skill, original).unwrap();
    fixture.startup();
}

#[test]
fn native_env_set_repairs_invalid_machine_skills_before_mandatory_reconciliation() {
    for skills in [
        json!("broken"),
        json!([{"name":"agenda", "path":"/unread-source"}]),
    ] {
        let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
        fixture.startup();
        let seed = fixture.run(&["env", "set", "agent_capabilities.skills=[]", "-w", "alpha"]);
        assert!(
            seed.status.success(),
            "{}",
            String::from_utf8_lossy(&seed.stderr)
        );
        let path = fixture.home.join(".config/brain/env.json");
        let mut registry = read_json(&path);
        registry["workspaces"]["alpha"]["env"]["agent_capabilities"] =
            json!({"skills":skills,"mcps":[]});
        write_json(&path, &registry);
        let blocked = fixture.run(&["server", "status"]);
        assert!(!blocked.status.success());
        fs::remove_file(fixture.root.join(".agents/skills/agenda/SKILL.md")).unwrap();
        let repaired = fixture.run(&["env", "set", "agent_capabilities.skills=[]", "-w", "alpha"]);
        assert!(
            repaired.status.success(),
            "{}",
            String::from_utf8_lossy(&repaired.stderr)
        );
        let after = read_json(&path);
        registry["workspaces"]["alpha"]["env"]["agent_capabilities"]["skills"] = json!([]);
        assert_eq!(after, registry);
        assert!(
            fixture
                .root
                .join(".agents/skills/agenda/SKILL.md")
                .is_file()
        );
        assert_eq!(
            String::from_utf8_lossy(&repaired.stdout)
                .matches("agent_capabilities.skills")
                .count(),
            1
        );
        fixture.startup();
    }
}
