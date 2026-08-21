use std::path::Path;

const TASK_FIELDS: &[&str] = &[
    "tag_styles",
    "today",
    "full_notes",
    "expanded_notes",
    "task_options",
    "all_tasks",
    "all_habits",
    "active_view",
    "base_tasks",
    "header",
    "query",
    "in_search",
    "matcher",
    "assignment",
    "assignment_filter",
    "visible_tasks",
    "task_line_ranges",
    "selected_task",
    "pending_count",
    "body_lines",
    "visual_row_offsets",
    "scroll",
    "last_inner_height",
    "last_content_rows",
];

const SHELL_FIELDS: &[&str] = &[
    "main_view",
    "focus",
    "panel_side",
    "brain_rect",
    "search",
    "logs_view",
    "active_brain_tab",
];

#[test]
fn app_owns_focused_task_and_shell_aggregates_instead_of_flat_state() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let app = std::fs::read_to_string(root.join("src/tui/mod.rs")).expect("read App source");
    let tasks =
        std::fs::read_to_string(root.join("src/tui/state/tasks.rs")).expect("read task state");
    let shell =
        std::fs::read_to_string(root.join("src/tui/state/shell.rs")).expect("read shell state");

    assert!(
        app.contains("tasks: TasksState"),
        "App must own one focused TasksState"
    );
    assert!(
        app.contains("shell: ShellState"),
        "App must own one focused ShellState"
    );

    for field in TASK_FIELDS {
        assert!(
            tasks.contains(&format!("{field}:")),
            "TasksState must own {field}"
        );
        assert!(
            !declares_field(&app, field),
            "App still owns flat task field {field}"
        );
    }
    for field in SHELL_FIELDS {
        assert!(
            shell.contains(&format!("{field}:")),
            "ShellState must own {field}"
        );
        assert!(
            !declares_field(&app, field),
            "App still owns flat shell field {field}"
        );
    }
}

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
    }

    assert!(
        leaks.is_empty(),
        "focused state representation leaked outside src/tui/state/:\n{}",
        leaks.join("\n")
    );
}

fn declares_field(source: &str, field: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with(&format!("{field}:"))
            || trimmed.starts_with(&format!("pub(crate) {field}:"))
    })
}

fn directly_accesses_field(source: &str, aggregate: &str, field: &str) -> bool {
    let compact: String = source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let needle = format!(".{aggregate}.{field}");
    compact.match_indices(&needle).any(|(at, _)| {
        compact[at + needle.len()..]
            .chars()
            .next()
            .is_some_and(|next| next != '(' && !next.is_ascii_alphanumeric() && next != '_')
    })
}
