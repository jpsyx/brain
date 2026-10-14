use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceManifest, WorkspaceName};
use std::fs;

fn workspace(home: &std::path::Path, root: &std::path::Path) -> WorkspaceContext {
    let id = WorkspaceId::new();
    WorkspaceManifest::new(id).write_new(root).unwrap();
    fs::write(
        root.join(".config/config.json"),
        r#"{"skills_auto_sync":false}"#,
    )
    .unwrap();
    WorkspaceContext::new(
        home,
        id,
        WorkspaceName::parse("alpha").unwrap(),
        root,
        "test-user",
        root,
    )
    .unwrap()
}
#[test]
fn compatibility_refresh_bypasses_optional_gate_and_repairs_missing_at_equal_version() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let ws = workspace(home.path(), root.path());
    super::resync_on_version_change(&ws);
    let todo = root.path().join(".agents/skills/todo/SKILL.md");
    assert!(
        todo.exists(),
        "mandatory revision must ignore routine preference"
    );
    fs::remove_file(&todo).unwrap();
    super::resync_on_version_change(&ws);
    assert!(
        todo.exists(),
        "equal version does not hide a missing artifact"
    );
    fs::create_dir_all(root.path().join(".config/extensions")).unwrap();
    fs::write(
        root.path().join(".config/extensions/todo.md"),
        "[todo:anchors]\noptional update\n",
    )
    .unwrap();
    super::resync_skills(&ws);
    assert!(
        !fs::read_to_string(&todo)
            .unwrap()
            .contains("optional update")
    );
    assert!(
        fs::read_to_string(root.path().join(".config/config.json"))
            .unwrap()
            .contains("false")
    );
}
#[test]
fn tui_startup_checks_receipt_without_duplicate_write_or_optional_source_reload() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let ws = workspace(home.path(), root.path());
    super::sync_for_startup(&ws);
    let todo = root.path().join(".agents/skills/todo/SKILL.md");
    let before = fs::metadata(&todo).unwrap().modified().unwrap();
    fs::create_dir_all(root.path().join(".config/extensions")).unwrap();
    fs::write(
        root.path().join(".config/extensions/todo.md"),
        "[todo:anchors]\nunrequested source\n",
    )
    .unwrap();
    super::sync_for_startup(&ws);
    assert_eq!(
        fs::metadata(&todo).unwrap().modified().unwrap(),
        before,
        "current startup must not reinstall"
    );
    assert!(
        !fs::read_to_string(todo)
            .unwrap()
            .contains("unrequested source")
    );
}
