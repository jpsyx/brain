use std::path::Path;

use super::support::{
    SHELL_FIELDS, TASK_FIELDS, TASKS_STATE_API, TASKS_STATE_TYPES, compact_signature,
    declares_function, directly_accesses_field, expected_links_plan_shape, function_signature,
    has_aliased_field_access, has_exact_named_shape, has_pure_direct_aggregate_forwarder,
    has_raw_aggregate_forwarder, public_impl_method_names, public_impl_method_signatures,
    public_state_type_names,
};

#[test]
fn aggregate_representation_does_not_leak_outside_its_owner() {
    let tui_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let state_root = tui_root.join("state");
    let mut leaks = Vec::new();

    for entry in walkdir::WalkDir::new(&tui_root) {
        let entry = entry.expect("walk TUI source");
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || path.starts_with(&state_root)
        {
            continue;
        }
        let source = std::fs::read_to_string(path).expect("read TUI source");
        for field in TASK_FIELDS {
            if directly_accesses_field(&source, "tasks", field) {
                leaks.push(format!("{}: tasks.{field}", path.display()));
            }
        }
        for field in SHELL_FIELDS {
            if directly_accesses_field(&source, "shell", field) {
                leaks.push(format!("{}: shell.{field}", path.display()));
            }
        }
        if has_aliased_field_access(&source, "tasks", TASK_FIELDS) {
            leaks.push(format!(
                "{}: aliased TasksState field access",
                path.display()
            ));
        }
        if has_aliased_field_access(&source, "shell", SHELL_FIELDS) {
            leaks.push(format!(
                "{}: aliased ShellState field access",
                path.display()
            ));
        }
        if has_raw_aggregate_forwarder(&source) {
            leaks.push(format!("{}: raw App aggregate forwarder", path.display()));
        }
        if has_pure_direct_aggregate_forwarder(&source) {
            leaks.push(format!(
                "{}: pure direct App aggregate forwarder",
                path.display()
            ));
        }
    }

    assert!(
        leaks.is_empty(),
        "focused state representation leaked outside src/tui/state/:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn aggregate_surfaces_and_consumers_stay_focused() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tasks = std::fs::read_to_string(root.join("src/tui/state/tasks.rs")).expect("task state");
    let shell = std::fs::read_to_string(root.join("src/tui/state/shell.rs")).expect("shell state");
    let logs = std::fs::read_to_string(root.join("src/tui/handlers/logs.rs")).expect("log handler");
    let task_handler =
        std::fs::read_to_string(root.join("src/tui/handlers/tasks_view.rs")).expect("task handler");
    let brain_renderer =
        std::fs::read_to_string(root.join("src/tui/draw/brain_panel.rs")).expect("brain renderer");

    let mut task_state_source = tasks.clone();
    for entry in walkdir::WalkDir::new(root.join("src/tui/state/tasks")) {
        let entry = entry.expect("walk task state source");
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("rs")
        {
            task_state_source.push('\n');
            task_state_source.push_str(
                &std::fs::read_to_string(entry.path()).expect("read task state child source"),
            );
        }
    }

    let mut api = public_impl_method_names(&task_state_source, "TasksState");
    api.sort_unstable();
    let mut expected_api = TASKS_STATE_API.to_vec();
    expected_api.sort_unstable();
    assert_eq!(api, expected_api, "TasksState semantic API drifted");

    let mut types = public_state_type_names(&task_state_source);
    types.sort_unstable();
    let mut expected_types = TASKS_STATE_TYPES.to_vec();
    expected_types.sort_unstable();
    assert_eq!(
        types, expected_types,
        "task state added or removed a public projection without guard review"
    );

    for (kind, name, expected) in [
        ("struct", "TasksPanelModel", "state:&'aTasksState,"),
        (
            "struct",
            "TaskAssignmentSnapshot",
            "pub(crate)mode:AssignmentUiMode,pub(crate)actor_id:&'aUserId,pub(crate)users:&'a[AssignmentUser],pub(crate)filter:Option<&'aUserId>,",
        ),
        (
            "struct",
            "DailyTriageNudge",
            "pub(crate)task_id:String,pub(crate)task_label:String,",
        ),
    ] {
        assert!(
            has_exact_named_shape(&task_state_source, kind, name, expected),
            "{name} fields, visibility, or types drifted"
        );
    }
    assert!(
        has_exact_named_shape(
            &task_state_source,
            "enum",
            "TaskLinksPlan",
            &expected_links_plan_shape()
        ),
        "TaskLinksPlan variants, fields, or types drifted"
    );

    for signature in public_impl_method_signatures(&task_state_source, "TasksState") {
        let Some((_, returned)) = signature.split_once("->") else {
            continue;
        };
        assert!(
            !returned.contains("&Task")
                && !returned.contains("&[Task]")
                && !returned.contains("&[Line")
                && !returned.contains("TasksRenderState")
                && !returned.contains("TaskRowsSnapshot")
                && !returned.contains("TaskTriageSnapshot"),
            "TasksState returns raw representation: {signature}"
        );
    }
    for (name, expected) in [
        (
            "selected_links_plan",
            "fnselected_links_plan(&self,linear_base:&str)->TaskLinksPlan",
        ),
        (
            "daily_triage_nudge",
            "fndaily_triage_nudge(&self,enabled:bool,disabled:bool,pattern:&str)->Option<DailyTriageNudge>",
        ),
        ("panel_model", "fnpanel_model(&self)->TasksPanelModel<'_>"),
    ] {
        assert_eq!(
            compact_signature(function_signature(&task_state_source, name)),
            expected,
            "unexpected focused signature for {name}"
        );
    }
    assert_eq!(
        compact_signature(function_signature(&task_state_source, "content")),
        "fncontent(&self)->implExactSizeIterator<Item=&'aLine<'static>>"
    );

    for getter in [
        "search_mut",
        "source_rows",
        "all_habits",
        "assignment",
        "assignment_filter",
        "body_lines",
    ] {
        assert!(
            !declares_function(&tasks, getter) && !declares_function(&shell, getter),
            "raw aggregate representation accessor remains: {getter}"
        );
    }
    assert!(
        root.join("src/tui/state/tasks/filter.rs").is_file(),
        "task matching must live beneath TasksState"
    );
    assert_eq!(
        compact_signature(function_signature(&logs, "handle_logs_key")),
        "fnhandle_logs_key(shell:&mutShellState,code:KeyCode,ctrl:bool)->bool"
    );
    assert_eq!(
        compact_signature(function_signature(&task_handler, "handle_search_key")),
        "fnhandle_search_key(tasks:&mutTasksState,code:KeyCode,ctrl:bool)->TaskSearchEffect"
    );
    assert_eq!(
        compact_signature(function_signature(&brain_renderer, "draw_brain")),
        "fndraw_brain(f:&mutFrame,context:&mutBrainPanelContext<'_>,area:Rect)"
    );
}
