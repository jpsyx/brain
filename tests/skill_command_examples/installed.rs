//! Installed guidance must retain customization and the complete CLI contract.
use std::fs;

use super::{parsing, workspace};

#[test]
fn installed_deletion_skill_contract() {
    let fixture = workspace::Fixture::new();
    let extension = fixture.root().join(".config/extensions/todo.md");
    fs::create_dir_all(extension.parent().unwrap()).unwrap();
    let source = "[todo:linear-backlog]\n\nSynthetic extension sentinel.\n";
    fs::write(&extension, source).unwrap();
    let before = fixture.canonical_bytes();
    let argv = ["brain", "skills", "sync", "--all-workspaces", "--json"];
    let outer = brain::cli::try_parse_from(argv).unwrap();
    assert!(
        matches!(outer.command, Some(brain::cli::Cmd::Skills(skills))
        if matches!(skills.action, Some(brain::cli::SkillsAction::Sync {
            root: None, all_workspaces: true, json: true })))
    );
    let report = fixture.json(&argv, 0);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read(&extension).unwrap(), source.as_bytes());
    assert_eq!(fixture.canonical_bytes(), before);

    let installed = fixture.root().join(".agents/skills");
    let read = |path: &str| fs::read_to_string(installed.join(path)).unwrap();
    let todo = read("todo/SKILL.md");
    for required in [
        "Synthetic extension sentinel.",
        "### Remove tasks",
        "brain tasks remove T17 --dry-run --json",
        "pass its `fingerprint` as `--expect-plan`",
        "These options\ndo not delete dependent tasks.",
        "Managed\ntriage habits remain protected.",
        "Never substitute an agent-tool cancellation",
    ] {
        assert!(
            todo.contains(required),
            "missing installed guidance: {required}"
        );
    }
    assert!(read("todo/references/commands.md").contains("--operation-status"));
    assert!(
        read("projects/references/task-project-link.md")
            .contains("report incomplete source removal")
    );
    assert!(read("triage/SKILL.md").contains("all consented IDs in one command"));
    assert!(read("projects/SKILL.md").contains("../todo/SKILL.md#remove-tasks"));
    assert!(
        read("todo/references/task-project-link.md")
            .contains("../../projects/references/task-project-link.md")
    );

    let mut examples = std::collections::BTreeSet::new();
    let mut templates = 0;
    for entry in walkdir::WalkDir::new(&installed).follow_links(false) {
        let entry = entry.unwrap();
        if entry.file_type().is_file()
            && entry.path().extension().is_some_and(|value| value == "md")
        {
            let markdown = fs::read_to_string(entry.path()).unwrap();
            let (found, count) = parsing::removal_examples(&markdown);
            examples.extend(found);
            templates += count;
        }
    }
    assert!(
        examples.len() >= 9,
        "installed removal examples: {examples:?}"
    );
    assert!(
        templates >= 4,
        "installed fingerprint and lifecycle templates"
    );
}
