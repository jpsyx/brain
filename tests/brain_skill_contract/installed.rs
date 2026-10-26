use std::fs;

use brain::skills::{install, layout::Layout};

#[test]
fn legacy_resource_hook_reaches_organize_once_without_changing_user_sources() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let extensions = root.join("extensions");
    fs::create_dir(&extensions).unwrap();
    let source = "[second-brain:reference-manager]\nResource integration sentinel.\n";
    fs::write(extensions.join("second-brain.md"), source).unwrap();
    let user = root.join(".agents/skills/user-owned");
    fs::create_dir_all(&user).unwrap();
    fs::write(user.join("SKILL.md"), "# Existing user skill\n").unwrap();
    let sources = install::Sources {
        extensions_dir: Some(extensions.clone()),
        plugins_dir: None,
    };
    install::sync(&Layout::under_root(root), &sources).unwrap();
    let built = root.join(".agents/skills");
    let organize = fs::read_to_string(built.join("organize/SKILL.md")).unwrap();
    assert_eq!(
        organize.matches("Resource integration sentinel.").count(),
        1
    );
    let insertion = organize.find("Resource integration sentinel.").unwrap();
    assert!(organize.find("## Add a resource").unwrap() < insertion);
    assert!(insertion < organize.find("## Retrieval").unwrap());
    let router = fs::read_to_string(built.join("second-brain/SKILL.md")).unwrap();
    assert!(!router.contains("Resource integration sentinel."));
    assert_eq!(
        fs::read_to_string(extensions.join("second-brain.md")).unwrap(),
        source
    );
    assert_eq!(
        fs::read_to_string(user.join("SKILL.md")).unwrap(),
        "# Existing user skill\n"
    );
    assert!(
        fs::read_to_string(built.join("organize/SKILL.md"))
            .unwrap()
            .contains("read its `AGENTS.md`")
    );
}

#[test]
fn all_legacy_agenda_hooks_land_once_in_their_semantic_sections() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let extensions = root.join("extensions");
    fs::create_dir(&extensions).unwrap();
    let source = "[todo:anchors]\nANCHOR SENTINEL\n[todo:calendar]\nCALENDAR SENTINEL\n[todo:cutoff]\nCUTOFF SENTINEL\n[todo:agenda-after-build]\nBUILD SENTINEL\n";
    fs::write(extensions.join("todo.md"), source).unwrap();
    install::sync(
        &Layout::under_root(root),
        &install::Sources {
            extensions_dir: Some(extensions.clone()),
            plugins_dir: None,
        },
    )
    .unwrap();
    let built = root.join(".agents/skills");
    let agenda = fs::read_to_string(built.join("agenda/SKILL.md")).unwrap();
    let todo = fs::read_to_string(built.join("todo/SKILL.md")).unwrap();
    for (section, sentinel, next) in [
        (
            "## Daily anchors",
            "ANCHOR SENTINEL",
            Some("## Calendar busy blocks"),
        ),
        (
            "## Calendar busy blocks",
            "CALENDAR SENTINEL",
            Some("## Work-hours cutoff"),
        ),
        (
            "## Work-hours cutoff",
            "CUTOFF SENTINEL",
            Some("## After build"),
        ),
        ("## After build", "BUILD SENTINEL", None),
    ] {
        assert_eq!(agenda.matches(sentinel).count(), 1);
        assert!(!todo.contains(sentinel));
        let position = agenda.find(sentinel).unwrap();
        assert!(agenda.find(section).unwrap() < position);
        if let Some(next) = next {
            assert!(position < agenda.find(next).unwrap());
        }
    }
    assert_eq!(
        fs::read(extensions.join("todo.md")).unwrap(),
        source.as_bytes()
    );
}
