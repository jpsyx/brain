//! Portable state must remain safe before this machine has package ownership.
use super::{
    installer::{compatible, refresh, target},
    *,
};

pub(super) fn modern_document() -> Vec<u8> {
    let source = Fixture::new(env!("CARGO_PKG_VERSION"));
    source.startup();
    [
        b"USER PREFIX\n".as_slice(),
        &std::fs::read(source.root.join("AGENTS.md")).unwrap(),
        b"USER SUFFIX WITHOUT NEWLINE",
    ]
    .concat()
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn refusal(f: &Fixture, executable: &Path, version: &str, diagnostic: &str) {
    let config = std::fs::read(f.root.join(".config/config.json")).ok();
    let agents = std::fs::read(f.root.join("AGENTS.md")).ok();
    for phase in ["preflight", "apply"] {
        let output = refresh(f, executable, version, phase);
        assert!(
            !output.status.success(),
            "{version} {phase} accepted incompatible portable state"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(f.root.join(".config/config.json")).ok(),
            config
        );
        assert_eq!(std::fs::read(f.root.join("AGENTS.md")).ok(), agents);
        assert!(!f.root.join(".agents").exists());
        assert!(!f.cache().join("skills-install/managed.json").exists());
    }
}

#[test]
fn canonical_only_scope_without_a_local_receipt_refuses_legacy_publication() {
    for version in ["0.101.0", "0.105.3"] {
        let f = Fixture::new(env!("CARGO_PKG_VERSION"));
        compatible(&f);
        write_json(
            &f.root.join(".config/config.json"),
            &json!({"allowed_skills":["agenda"], "skills_auto_sync":false}),
        );
        refusal(&f, &target(&f, version), version, "equivalent scope");
    }
}

#[test]
fn portable_modern_instructions_without_local_ownership_refuse_legacy_publication() {
    let document = modern_document();
    for version in ["0.101.0", "0.105.3"] {
        let f = Fixture::new(env!("CARGO_PKG_VERSION"));
        compatible(&f);
        std::fs::write(f.root.join("AGENTS.md"), &document).unwrap();
        refusal(
            &f,
            &target(&f, version),
            version,
            "local instruction ownership",
        );
        success(&refresh(
            &f,
            Path::new(env!("CARGO_BIN_EXE_brain")),
            env!("CARGO_PKG_VERSION"),
            "apply",
        ));
        assert_eq!(std::fs::read(f.root.join("AGENTS.md")).unwrap(), document);
        for phase in ["preflight", "apply"] {
            success(&refresh(&f, &target(&f, version), version, phase));
        }
        let restored = std::fs::read_to_string(f.root.join("AGENTS.md")).unwrap();
        assert!(!restored.contains("brain:managed-workspace"));
        assert!(restored.starts_with("USER PREFIX\n"));
        assert!(restored.ends_with("USER SUFFIX WITHOUT NEWLINE"));
        assert!(restored.contains(include_str!(
            "../../templates/workspace/legacy/AGENTS-v0.105.3.md"
        )));
    }
}

#[test]
fn legacy_only_and_plugin_selections_without_history_remain_unchanged() {
    for version in ["0.101.0", "0.105.3"] {
        let f = Fixture::new(env!("CARGO_PKG_VERSION"));
        compatible(&f);
        let config = b"{\"allowed_skills\":[\"todo\",\"second-brain\",\"Custom\"],\"skills_auto_sync\":false}\n";
        std::fs::write(f.root.join(".config/config.json"), config).unwrap();
        let document = [
            b"USER PREFIX\n".as_slice(),
            include_bytes!("../../templates/workspace/legacy/AGENTS-v0.105.3.md"),
            b"USER SUFFIX WITHOUT NEWLINE",
        ]
        .concat();
        std::fs::write(f.root.join("AGENTS.md"), &document).unwrap();
        let executable = target(&f, version);
        for phase in ["preflight", "apply"] {
            success(&refresh(&f, &executable, version, phase));
            assert_eq!(
                std::fs::read(f.root.join(".config/config.json")).unwrap(),
                config
            );
            assert_eq!(std::fs::read(f.root.join("AGENTS.md")).unwrap(), document);
        }
    }
}

#[test]
fn current_target_adopts_portable_instructions_without_broadening_canonical_scope() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    compatible(&f);
    let document = modern_document();
    std::fs::write(f.root.join("AGENTS.md"), &document).unwrap();
    let config = b"{\"allowed_skills\":[\"agenda\"],\"skills_auto_sync\":false}\n";
    std::fs::write(f.root.join(".config/config.json"), config).unwrap();
    for phase in ["preflight", "apply"] {
        success(&refresh(
            &f,
            Path::new(env!("CARGO_BIN_EXE_brain")),
            env!("CARGO_PKG_VERSION"),
            phase,
        ));
        assert_eq!(
            std::fs::read(f.root.join(".config/config.json")).unwrap(),
            config
        );
        assert_eq!(std::fs::read(f.root.join("AGENTS.md")).unwrap(), document);
    }
    let manifest = read_json(&f.cache().join("skills-install/managed.json"));
    assert!(manifest["package"]["instructions"].is_object());
    assert!(manifest["package"]["extension_snapshot"].is_object());
    refusal_after_current_adoption(&f);
}

fn refusal_after_current_adoption(f: &Fixture) {
    let manifest = f.cache().join("skills-install/managed.json");
    let before = std::fs::read(&manifest).unwrap();
    let output = refresh(f, &target(f, "0.105.3"), "0.105.3", "preflight");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("equivalent scope"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(manifest).unwrap(), before);
}

#[test]
#[ignore = "requires explicitly supplied audited 0.101.0 and 0.105.3 binaries"]
fn audited_targets_check_receiptless_scope_and_instruction_ownership() {
    let document = modern_document();
    for (version, key) in [
        ("0.101.0", "BRAIN_TEST_LEGACY_BINARY"),
        ("0.105.3", "BRAIN_TEST_105_BINARY"),
    ] {
        let old = PathBuf::from(std::env::var_os(key).expect("audited binary fixture"));
        for modern in [false, true] {
            let f = Fixture::new(env!("CARGO_PKG_VERSION"));
            compatible(&f);
            let diagnostic = if modern {
                std::fs::write(f.root.join("AGENTS.md"), &document).unwrap();
                "local instruction ownership"
            } else {
                write_json(
                    &f.root.join(".config/config.json"),
                    &json!({"allowed_skills":["agenda"], "skills_auto_sync":false}),
                );
                "equivalent scope"
            };
            refusal(&f, &old, version, diagnostic);
        }
        let f = Fixture::new(version);
        compatible(&f);
        let config = b"{\"allowed_skills\":[\"todo\",\"second-brain\",\"custom\"],\"skills_auto_sync\":false}\n";
        std::fs::write(f.root.join(".config/config.json"), config).unwrap();
        let legacy = [
            b"USER PREFIX\n".as_slice(),
            include_bytes!("../../templates/workspace/legacy/AGENTS-v0.105.3.md"),
            b"USER SUFFIX WITHOUT NEWLINE",
        ]
        .concat();
        std::fs::write(f.root.join("AGENTS.md"), &legacy).unwrap();
        let plugin = f.root.join(".config/plugins/custom/SKILL.md");
        std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
        let plugin_bytes =
            b"---\nname: custom\ndescription: Fixture capability\n---\nCUSTOM PLUGIN CAPABILITY\n";
        std::fs::write(&plugin, plugin_bytes).unwrap();
        for phase in ["preflight", "apply"] {
            success(&refresh(&f, &old, version, phase));
            assert_eq!(
                std::fs::read(f.root.join(".config/config.json")).unwrap(),
                config
            );
            assert_eq!(std::fs::read(f.root.join("AGENTS.md")).unwrap(), legacy);
            assert_eq!(std::fs::read(&plugin).unwrap(), plugin_bytes);
        }
        success(&super::downgrade::ordinary(
            &f,
            &old,
            &["-w", "alpha", "skills", "sync"],
        ));
        let mut historical_config: Value = serde_json::from_slice(config).unwrap();
        historical_config["access_mode"] = "unrestricted".into();
        assert_eq!(
            read_json(&f.root.join(".config/config.json")),
            historical_config
        );
        assert_eq!(std::fs::read(f.root.join("AGENTS.md")).unwrap(), legacy);
        assert_eq!(
            std::fs::read(f.root.join(".agents/skills/custom/SKILL.md")).unwrap(),
            plugin_bytes
        );
    }
}
