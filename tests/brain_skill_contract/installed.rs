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
