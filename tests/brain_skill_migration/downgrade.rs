use super::{
    installer::{compatible, refresh},
    *,
};

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
pub(super) fn ordinary(f: &Fixture, executable: &Path, args: &[&str]) -> Output {
    Command::new(executable)
        .args(args)
        .env_clear()
        .env("HOME", &f.home)
        .env("XDG_CONFIG_HOME", f.home.join(".config"))
        .env("XDG_CACHE_HOME", f.home.join(".cache"))
        .env("XDG_DATA_HOME", f.home.join(".local/share"))
        .env("XDG_RUNTIME_DIR", f.temp.path().join("runtime"))
        .env("TMPDIR", f.temp.path())
        .env("PATH", "/usr/bin:/bin")
        .env("NO_COLOR", "1")
        .current_dir(&f.root)
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires explicitly supplied audited 0.101.0 and 0.105.3 binaries"]
fn audited_targets_upgrade_down_ordinary_sync_and_reupgrade_complete_packages() {
    for (version, key, revision) in [
        ("0.101.0", "BRAIN_TEST_LEGACY_BINARY", 0),
        ("0.105.3", "BRAIN_TEST_105_BINARY", 1),
    ] {
        let old = PathBuf::from(std::env::var_os(key).expect("audited binary fixture"));
        let current = Path::new(env!("CARGO_BIN_EXE_brain"));
        let f = Fixture::new(version);
        compatible(&f);
        std::fs::write(
            f.root.join("AGENTS.md"),
            include_bytes!("../../templates/workspace/legacy/AGENTS-v0.105.3.md"),
        )
        .unwrap();
        write_json(
            &f.root.join(".config/config.json"),
            &json!({"allowed_skills":["todo"],"skills_auto_sync":false}),
        );
        success(&ordinary(&f, &old, &["-w", "alpha", "skills", "sync"]));
        for phase in ["preflight", "apply"] {
            success(&refresh(&f, current, env!("CARGO_PKG_VERSION"), phase));
        }
        assert_eq!(
            read_json(&f.root.join(".config/config.json"))["allowed_skills"],
            json!(["todo", "agenda"])
        );
        let directory = f.root.join(".config/extensions");
        std::fs::create_dir_all(&directory).unwrap();
        let canonical = b"[agenda:anchors]\nRETAINED CANONICAL ANCHOR\n";
        std::fs::write(directory.join("agenda.md"), canonical).unwrap();
        std::fs::write(directory.join("workspace.md"), "ALWAYS ON CONTEXT\n").unwrap();
        success(&refresh(&f, current, env!("CARGO_PKG_VERSION"), "apply"));
        let agents = f.root.join("AGENTS.md");
        let modern = std::fs::read(&agents).unwrap();
        std::fs::write(
            &agents,
            [b"USER PREFIX\n".as_slice(), &modern, b"USER SUFFIX\n"].concat(),
        )
        .unwrap();
        let config_path = f.root.join(".config/config.json");
        let mut config = read_json(&config_path);
        config["agenda_dir"] = "custom-agenda".into();
        write_json(&config_path, &config);
        success(&refresh(&f, &old, version, "preflight"));
        assert!(!directory.join("todo.md").exists());
        success(&f.run(&[
            "__migrate",
            "--from-version",
            env!("CARGO_PKG_VERSION"),
            "--to-version",
            version,
            "--installer-refresh-protocol",
            "1",
        ]));
        success(&refresh(&f, &old, version, "apply"));
        let down = std::fs::read_to_string(&agents).unwrap();
        assert!(down.starts_with("USER PREFIX\n"));
        assert!(down.ends_with("USER SUFFIX\n"));
        assert!(down.contains(include_str!(
            "../../templates/workspace/legacy/AGENTS-v0.105.3.md"
        )));
        assert!(!down.contains("<!-- brain:managed"));
        let manifest_path = f.cache().join("skills-install/managed.json");
        let manifest = read_json(&manifest_path);
        assert_eq!(manifest["schema_version"], 1);
        assert_eq!(manifest["compatibility_revision"], revision);
        assert!(manifest.get("package").is_none());
        let config = read_json(&config_path);
        assert_eq!(config["allowed_skills"], json!(["todo"]));
        assert_eq!(config["agenda_dir"], "custom-agenda");
        assert_eq!(
            std::fs::read(directory.join("agenda.md")).unwrap(),
            canonical
        );
        success(&ordinary(&f, &old, &["-w", "alpha", "skills", "sync"]));
        assert!(
            std::fs::read_to_string(f.root.join(".agents/skills/todo/SKILL.md"))
                .unwrap()
                .contains("RETAINED CANONICAL ANCHOR")
        );
        assert!(
            std::fs::read_to_string(f.root.join(".agents/skills/second-brain/SKILL.md"))
                .unwrap()
                .contains("ALWAYS ON CONTEXT")
        );
        success(&refresh(
            &f,
            current,
            env!("CARGO_PKG_VERSION"),
            "preflight",
        ));
        success(&refresh(&f, current, env!("CARGO_PKG_VERSION"), "apply"));
        f.startup();
        let agenda =
            std::fs::read_to_string(f.root.join(".agents/skills/agenda/SKILL.md")).unwrap();
        assert_eq!(agenda.matches("RETAINED CANONICAL ANCHOR").count(), 1);
        let up = std::fs::read_to_string(&agents).unwrap();
        assert_eq!(up.matches("ALWAYS ON CONTEXT").count(), 1);
        assert!(up.starts_with("USER PREFIX\n"));
        assert_eq!(up.matches("USER SUFFIX\n").count(), 1);
        assert!(!up.contains("## Things to avoid"));
        let manifest = read_json(&manifest_path);
        assert_eq!(manifest["schema_version"], 2);
        assert!(manifest["package"]["instructions"].is_object());
        assert!(manifest["package"]["extension_snapshot"].is_object());
        let backups = f.cache().join("skills-install/package-backups");
        assert!(std::fs::read_dir(backups).unwrap().next().is_some());
    }
}
