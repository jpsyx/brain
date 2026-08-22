use std::path::Path;

use super::support::{
    APP_COMPOSITION_FIELDS, BRAIN_FIELDS, CONTEXT_FIELDS, SERVICE_FIELDS, SHELL_FIELDS,
    STATUS_FIELDS, TASK_FIELDS, extract_struct_body, field_declaration_count, field_is_private,
    field_type, struct_field_names,
};

#[test]
fn app_is_an_eight_field_composition_root_with_one_owner_per_remaining_invariant() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let app = std::fs::read_to_string(root.join("src/tui/mod.rs")).expect("read App source");
    let app_body = extract_struct_body(&app, "App").expect("App body");
    let fields = struct_field_names(app_body);

    assert!(
        fields.len() <= 10,
        "App must stay at or below ten intentional fields, found {}: {fields:?}",
        fields.len()
    );
    assert_eq!(
        fields,
        APP_COMPOSITION_FIELDS
            .iter()
            .map(|(field, _)| (*field).to_owned())
            .collect::<Vec<_>>(),
        "App composition fields drifted"
    );
    for (field, expected_type) in APP_COMPOSITION_FIELDS {
        assert!(
            field_is_private(app_body, field),
            "App.{field} must be private"
        );
        assert_eq!(
            field_type(app_body, field),
            Some((*expected_type).to_owned()),
            "App.{field} owner type drifted"
        );
    }

    for (path, owner, expected_fields) in [
        ("src/tui/state/context.rs", "AppContext", CONTEXT_FIELDS),
        ("src/tui/state/brain.rs", "BrainPanelState", BRAIN_FIELDS),
        ("src/tui/state/services.rs", "AppServices", SERVICE_FIELDS),
        ("src/tui/state/status.rs", "StatusState", STATUS_FIELDS),
    ] {
        let source = std::fs::read_to_string(root.join(path)).expect("read focused state owner");
        let body = extract_struct_body(&source, owner).expect("focused state body");
        assert_eq!(
            struct_field_names(body),
            expected_fields
                .iter()
                .map(|field| (*field).to_owned())
                .collect::<Vec<_>>(),
            "{owner} ownership drifted"
        );
        for field in expected_fields {
            assert!(
                field_is_private(body, field),
                "{owner}.{field} must be private"
            );
            assert_eq!(
                field_declaration_count(app_body, field),
                usize::from(
                    APP_COMPOSITION_FIELDS
                        .iter()
                        .any(|(app_field, _)| app_field == field)
                ),
                "{owner}.{field} was flattened back onto App"
            );
        }
    }
}

#[test]
fn app_owns_focused_task_and_shell_aggregates_instead_of_flat_state() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let app = std::fs::read_to_string(root.join("src/tui/mod.rs")).expect("read App source");
    let tasks =
        std::fs::read_to_string(root.join("src/tui/state/tasks.rs")).expect("read task state");
    let shell =
        std::fs::read_to_string(root.join("src/tui/state/shell.rs")).expect("read shell state");
    let app_body = extract_struct_body(&app, "App").expect("App body");
    let tasks_body = extract_struct_body(&tasks, "TasksState").expect("TasksState body");
    let shell_body = extract_struct_body(&shell, "ShellState").expect("ShellState body");

    assert_eq!(field_declaration_count(app_body, "tasks"), 1);
    assert_eq!(field_declaration_count(app_body, "shell"), 1);
    assert!(field_is_private(app_body, "tasks"));
    assert!(field_is_private(app_body, "shell"));
    assert_eq!(field_type(app_body, "tasks"), Some("TasksState".to_owned()));
    assert_eq!(field_type(app_body, "shell"), Some("ShellState".to_owned()));

    for field in TASK_FIELDS {
        assert_eq!(
            field_declaration_count(tasks_body, field),
            1,
            "TasksState.{field}"
        );
        assert!(
            field_is_private(tasks_body, field),
            "TasksState.{field} must be private"
        );
        assert_eq!(field_declaration_count(app_body, field), 0, "App.{field}");
        assert_eq!(
            field_declaration_count(shell_body, field),
            0,
            "ShellState.{field}"
        );
    }
    for field in SHELL_FIELDS {
        assert_eq!(
            field_declaration_count(shell_body, field),
            1,
            "ShellState.{field}"
        );
        assert!(
            field_is_private(shell_body, field),
            "ShellState.{field} must be private"
        );
        assert_eq!(field_declaration_count(app_body, field), 0, "App.{field}");
        assert_eq!(
            field_declaration_count(tasks_body, field),
            0,
            "TasksState.{field}"
        );
    }
}
