use super::{
    installer::{compatible, refresh, target},
    *,
};

#[test]
fn identical_mixed_sources_deduplicate_and_old_client_edits_conflict_without_data_loss() {
    let f = Fixture::new("0.105.3");
    compatible(&f);
    f.startup();
    let directory = f.root.join(".config/extensions");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("agenda.md"),
        "[agenda:anchors]\nSHARED ANCHOR\n",
    )
    .unwrap();
    let old = target(&f, "0.101.0");
    let output = refresh(&f, &old, "0.101.0", "apply");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let canonical = std::fs::read(directory.join("agenda.md")).unwrap();
    let legacy = std::fs::read(directory.join("todo.md")).unwrap();
    let current = Path::new(env!("CARGO_BIN_EXE_brain"));
    let output = refresh(&f, current, env!("CARGO_PKG_VERSION"), "apply");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join(".agents/skills/agenda/SKILL.md"))
            .unwrap()
            .matches("SHARED ANCHOR")
            .count(),
        1
    );
    let output = refresh(&f, &old, "0.101.0", "apply");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::write(
        directory.join("todo.md"),
        "[todo:anchors]\nEDITED BY OLD CLIENT\n",
    )
    .unwrap();
    let before = std::fs::read(f.root.join("AGENTS.md")).unwrap();
    for phase in ["preflight", "apply"] {
        let output = refresh(&f, current, env!("CARGO_PKG_VERSION"), phase);
        assert!(!output.status.success());
        assert_eq!(
            std::fs::read(directory.join("agenda.md")).unwrap(),
            canonical
        );
        assert_eq!(
            std::fs::read(directory.join("todo.md")).unwrap(),
            b"[todo:anchors]\nEDITED BY OLD CLIENT\n"
        );
        assert_eq!(std::fs::read(f.root.join("AGENTS.md")).unwrap(), before);
    }
    assert_ne!(legacy, b"[todo:anchors]\nEDITED BY OLD CLIENT\n");
}

#[test]
fn narrowed_scope_and_conflicting_sources_refuse_down_without_publication() {
    for fault in ["capability", "hook", "mixed"] {
        let f = Fixture::new("0.105.3");
        compatible(&f);
        f.startup();
        let before = std::fs::read(f.root.join("AGENTS.md")).unwrap();
        let directory = f.root.join(".config/extensions");
        std::fs::create_dir_all(&directory).unwrap();
        match fault {
            "capability" => write_json(
                &f.root.join(".config/config.json"),
                &json!({"allowed_skills":["agenda"]}),
            ),
            "hook" => {
                std::fs::write(directory.join("projects.md"), "NARROW PROJECT CONTEXT\n").unwrap();
            }
            _ => {
                std::fs::write(directory.join("agenda.md"), "[agenda:anchors]\nCANONICAL\n")
                    .unwrap();
                std::fs::write(directory.join("todo.md"), "[todo:anchors]\nDIFFERENT\n").unwrap();
            }
        }
        let old = target(&f, "0.101.0");
        let output = refresh(&f, &old, "0.101.0", "preflight");
        assert!(!output.status.success(), "{fault}");
        assert_eq!(std::fs::read(f.root.join("AGENTS.md")).unwrap(), before);
    }
}
