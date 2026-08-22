use std::path::Path;

const TASKS_STATE_API: &[&str] = &[
    "active_view",
    "advance_day",
    "append_query",
    "assignment_snapshot",
    "clear_active_filters",
    "clear_count",
    "clear_query",
    "collapse_notes",
    "contains_task_named",
    "current_has_notes",
    "current_is_habit",
    "current_notes_expanded",
    "current_task_id",
    "cycle_view_next",
    "cycle_view_prev",
    "daily_triage_date",
    "daily_triage_nudge",
    "enter_search",
    "expand_notes",
    "has_active_filter",
    "is_searching",
    "leave_search",
    "max_scroll",
    "new",
    "panel_model",
    "pop_query",
    "push_count_digit",
    "query_is_empty",
    "query_text",
    "replace_rows",
    "scroll_offset",
    "select_first",
    "select_last",
    "select_next",
    "select_prev",
    "selected_identity",
    "selected_link_kind",
    "selected_links_plan",
    "selection_band_rect",
    "set_assignment_filter",
    "set_view",
    "take_count",
    "tasks_per_page",
    "toggle_notes",
    "update_body_layout",
    "validate_removal",
    "visible_count",
];

const TASKS_STATE_TYPES: &[&str] = &[
    "DailyTriageNudge",
    "TaskAssignmentSnapshot",
    "TaskLinksPlan",
    "TasksPanelModel",
    "TasksState",
    "TasksStateInit",
];

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

const APP_COMPOSITION_FIELDS: &[(&str, &str)] = &[
    ("context", "AppContext"),
    ("tasks", "TasksState"),
    ("brain", "BrainPanelState"),
    ("shell", "ShellState"),
    ("overlay", "Option<Overlay>"),
    ("services", "AppServices"),
    ("status", "StatusState"),
    ("receiver", "crate::tui::receiver::ReceiverRuntime"),
];

const CONTEXT_FIELDS: &[&str] = &[
    "command",
    "config",
    "agent_kind",
    "agent_command",
    "csv_path",
    "brain_root",
    "db_path",
    "log_path",
    "server_ingress",
    "server_local_capability",
];

const BRAIN_FIELDS: &[&str] = &[
    "main",
    "brain_turn_active",
    "skill_sessions",
    "next_session_tab_id",
    "configured_skill_sessions",
    "instance",
    "interactive_actor",
    "session_actor",
    "brain_transport_override",
    "session_done_url_override",
    "session_transport_override",
];

const SERVICE_FIELDS: &[&str] = &[
    "agenda_runner",
    "open_runner",
    "db",
    "receiver_sync_runtime",
];

const STATUS_FIELDS: &[&str] = &[
    "triage_day",
    "triage_gate",
    "skip_daily_triage_check",
    "flash",
    "persistent_warning",
    "alert",
    "sync_status",
    "sync_status_next_poll",
    "last_seen_downstream_id",
];

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

#[test]
fn guard_helpers_cover_visibility_duplicates_aliases_and_forwarders() {
    let alternate_visibility = r"
        struct App {
            pub(super) query: String,
        }
    ";
    let body = extract_struct_body(alternate_visibility, "App").expect("App body");
    assert_eq!(field_declaration_count(body, "query"), 1);
    assert!(!field_is_private(body, "query"));
    assert_eq!(field_type(body, "query"), Some("String".to_owned()));

    let duplicate_visibility = r"
        struct App {
            pub(super) query: String,
            pub(in crate::tui) query: String,
        }
    ";
    let body = extract_struct_body(duplicate_visibility, "App").expect("App body");
    assert_eq!(field_declaration_count(body, "query"), 2);

    let exact_panel = r"
        pub(crate) struct TasksPanelModel<'a> {
            state: &'a TasksState,
        }
    ";
    let raw_assignment_field = r"
        pub(crate) struct TaskAssignmentSnapshot<'a> {
            pub(crate) mode: AssignmentUiMode,
            pub(crate) actor_id: &'a UserId,
            pub(crate) users: &'a [AssignmentUser],
            pub(crate) filter: Option<&'a UserId>,
            pub(crate) task: &'a Task,
        }
    ";
    let extra_panel_field = r"
        pub(crate) struct TasksPanelModel<'a> {
            state: &'a TasksState,
            count: usize,
        }
    ";
    let raw_nudge_field = r"
        pub(crate) struct DailyTriageNudge {
            pub(crate) task_id: String,
            pub(crate) task_label: String,
            pub(crate) habit: Task,
        }
    ";
    let extra_links_variant = r"
        pub(crate) enum TaskLinksPlan {
            None,
            Open { url: String },
            Choose { task_id: String, links: Vec<Link> },
            Raw(Task),
        }
    ";
    assert!(has_exact_named_shape(
        exact_panel,
        "struct",
        "TasksPanelModel",
        "state:&'aTasksState,"
    ));
    assert!(!has_exact_named_shape(
        raw_assignment_field,
        "struct",
        "TaskAssignmentSnapshot",
        "pub(crate)mode:AssignmentUiMode,pub(crate)actor_id:&'aUserId,pub(crate)users:&'a[AssignmentUser],pub(crate)filter:Option<&'aUserId>,"
    ));
    assert!(!has_exact_named_shape(
        extra_panel_field,
        "struct",
        "TasksPanelModel",
        "state:&'aTasksState,"
    ));
    assert!(!has_exact_named_shape(
        raw_nudge_field,
        "struct",
        "DailyTriageNudge",
        "pub(crate)task_id:String,pub(crate)task_label:String,"
    ));
    assert!(!has_exact_named_shape(
        extra_links_variant,
        "enum",
        "TaskLinksPlan",
        &expected_links_plan_shape()
    ));

    assert!(directly_accesses_field(
        "fn leak(app: &App) { let _ = &app.tasks.query; }",
        "tasks",
        "query"
    ));

    let alias_leak = r"
        fn leak(app: &App) {
            let local = &app.tasks;
            let _ = &local.query;
        }
    ";
    assert!(has_aliased_field_access(alias_leak, "tasks", &["query"]));

    let multiline_typed_parenthesized_alias = r"
        fn leak(app: &App) {
            let local: &TasksState = (
                &app.tasks
            );
            let _ = &local.query;
        }
    ";
    assert!(has_aliased_field_access(
        multiline_typed_parenthesized_alias,
        "tasks",
        &["query"]
    ));

    let forwarding = r"
        impl App {
            fn query(&self) -> &str { self.tasks.query_text() }
        }
    ";
    assert!(has_raw_aggregate_forwarder(forwarding));

    let multi_statement_forwarding = r"
        impl App {
            fn query(&self) -> &str {
                let state: &TasksState = (&self.tasks);
                state.query_text()
            }
        }
    ";
    assert!(has_raw_aggregate_forwarder(multi_statement_forwarding));

    let intermediate_forwarding = r"
        impl App {
            fn query(&self) -> &str {
                let state: &TasksState = (&self.tasks);
                let value: &str = ((state.query_text()));
                ((value))
            }
        }
    ";
    assert!(has_raw_aggregate_forwarder(intermediate_forwarding));

    let aliased_raw_return = r"
        type BorrowedValue<'a> = &'a str;
        type RenamedValue<'a> = BorrowedValue<'a>;

        impl App {
            fn query(&self) -> RenamedValue<'_> {
                self.tasks.query_text()
            }
        }
    ";
    assert!(has_raw_aggregate_forwarder(aliased_raw_return));

    let renamed_context_forwarder = r"
        impl App {
            fn endpoint(&self, port: u16) -> String {
                self.context.session_done_url(port)
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        renamed_context_forwarder
    ));

    let chained_brain_forwarder = r"
        impl App {
            fn panel_open(&self) -> bool {
                self.brain.main_controller().is_some()
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(chained_brain_forwarder));

    let aliased_context_forwarder = r"
        impl App {
            fn endpoint(&self, port: u16) -> String {
                let context = &self.context;
                context.session_done_url(port)
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        aliased_context_forwarder
    ));

    let propagated_typed_parenthesized_brain_forwarder = r"
        impl App {
            fn panel_open(&self) -> bool {
                let state: &BrainPanelState = ((&self.brain));
                let controller = (state.main_controller());
                ((controller.is_some()))
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        propagated_typed_parenthesized_brain_forwarder
    ));

    let dead_aliases_before_context_forwarder = r"
        impl App {
            fn endpoint(&self, port: u16) -> String {
                let unused_tasks = &self.tasks;
                let unused_shell = &self.shell;
                let context = &self.context;
                context.session_done_url(port)
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        dead_aliases_before_context_forwarder
    ));

    let shadowed_alias_uses_the_last_binding = r"
        impl App {
            fn endpoint(&self, port: u16) -> String {
                let owner = &self.brain;
                let owner: &AppContext = ((&self.context));
                owner.session_done_url(port)
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        shadowed_alias_uses_the_last_binding
    ));

    let cross_aggregate_mediator = r"
        impl App {
            fn active_tab(&self) -> BrainTab {
                self.shell.active_brain_tab(&self.brain.skill_session_tab_ids())
            }
        }
    ";
    assert!(!has_pure_direct_aggregate_forwarder(
        cross_aggregate_mediator
    ));

    let aliased_cross_aggregate_mediator = r"
        impl App {
            fn active_tab(&self) -> BrainTab {
                let shell = &self.shell;
                let brain: &BrainPanelState = ((&self.brain));
                shell.active_brain_tab(&brain.skill_session_tab_ids())
            }
        }
    ";
    assert!(!has_pure_direct_aggregate_forwarder(
        aliased_cross_aggregate_mediator
    ));
}

fn directly_accesses_field(source: &str, aggregate: &str, field: &str) -> bool {
    let compact: String = mask_non_code(source)
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

fn extract_struct_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    extract_named_body(source, "struct", name)
}

fn extract_named_body<'a>(source: &'a str, kind: &str, name: &str) -> Option<&'a str> {
    let masked = mask_non_code(source);
    let mut offset = 0;
    while let Some(relative) = masked[offset..].find(kind) {
        let start = offset + relative;
        if !token_at(&masked, start, kind) {
            offset = start + kind.len();
            continue;
        }
        let mut cursor = start + kind.len();
        cursor += masked[cursor..].len() - masked[cursor..].trim_start().len();
        let end = masked[cursor..]
            .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .map_or(masked.len(), |length| cursor + length);
        if &masked[cursor..end] == name {
            let open = masked[end..].find('{').map(|relative| end + relative)?;
            let close = matching_brace(&masked, open)?;
            return Some(&source[open + 1..close]);
        }
        offset = end;
    }
    None
}

fn field_declaration_count(body: &str, field: &str) -> usize {
    field_declarations(body, field).len()
}

fn struct_field_names(body: &str) -> Vec<String> {
    let masked = mask_non_code(body);
    body.lines()
        .zip(masked.lines())
        .filter_map(|(_, code)| {
            let code = code.trim();
            if code.is_empty() || code.starts_with('#') {
                return None;
            }
            let colon = field_separator(code)?;
            code[..colon]
                .split_whitespace()
                .next_back()
                .map(str::to_owned)
        })
        .collect()
}

fn field_is_private(body: &str, field: &str) -> bool {
    let declarations = field_declarations(body, field);
    declarations.len() == 1 && declarations[0] == field
}

fn field_type(body: &str, field: &str) -> Option<String> {
    let masked = mask_non_code(body);
    body.lines()
        .zip(masked.lines())
        .find_map(|(original, code)| {
            let code = code.trim();
            let colon = field_separator(code)?;
            let left = &code[..colon];
            if left.split_whitespace().next_back() != Some(field) {
                return None;
            }
            let original = original.trim();
            let original_colon = field_separator(original)?;
            Some(
                original[original_colon + 1..]
                    .trim()
                    .trim_end_matches(',')
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect(),
            )
        })
}

fn field_declarations<'a>(body: &'a str, field: &str) -> Vec<&'a str> {
    let masked = mask_non_code(body);
    body.lines()
        .zip(masked.lines())
        .filter_map(|(original, code)| {
            let code = code.trim();
            if code.is_empty() || code.starts_with('#') {
                return None;
            }
            let colon = field_separator(code)?;
            let left = &code[..colon];
            (left.split_whitespace().next_back() == Some(field))
                .then(|| original.trim()[..colon].trim())
        })
        .collect()
}

fn field_separator(code: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut paren_depth = 0_usize;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b':' if paren_depth == 0
                && bytes.get(index.wrapping_sub(1)) != Some(&b':')
                && bytes.get(index + 1) != Some(&b':') =>
            {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

fn has_aliased_field_access(source: &str, aggregate: &str, fields: &[&str]) -> bool {
    let tokens = code_tokens(source);
    let aliases = aggregate_aliases(&tokens, aggregate);
    aliases.iter().any(|alias| {
        tokens.windows(3).enumerate().any(|(index, window)| {
            window[0] == *alias
                && window[1] == "."
                && fields.contains(&window[2])
                && tokens.get(index + 3) != Some(&"(")
        })
    })
}

fn has_raw_aggregate_forwarder(source: &str) -> bool {
    let masked = mask_non_code(source);
    impl_app_ranges(&masked).into_iter().any(|(open, close)| {
        method_ranges(&masked, open, close)
            .into_iter()
            .any(|(fn_start, body_open, body_close)| {
                let signature = compact_signature(&source[fn_start..body_open]);
                let Some((_, returned)) = signature.split_once("->") else {
                    return false;
                };
                if !representation_like_return(source, returned) {
                    return false;
                }
                let body = &source[body_open + 1..body_close];
                ["tasks", "shell"].into_iter().any(|aggregate| {
                    let tokens = code_tokens(body);
                    let statements = top_level_statements(&tokens);
                    let Some((last, preceding)) = statements.split_last() else {
                        return false;
                    };
                    let mut aliases = Vec::new();
                    for statement in preceding {
                        let Some(alias) = tainted_alias_from_let(statement, aggregate, &aliases)
                        else {
                            return false;
                        };
                        aliases.push(alias);
                    }
                    forwarded_expression(last, aggregate, &aliases)
                })
            })
    })
}

fn has_pure_direct_aggregate_forwarder(source: &str) -> bool {
    const APP_OWNERS: &[&str] = &[
        "context", "tasks", "brain", "shell", "overlay", "services", "status", "receiver",
    ];
    const GUARDED_OWNERS: &[&str] = &["context", "brain"];

    let masked = mask_non_code(source);
    impl_app_ranges(&masked).into_iter().any(|(open, close)| {
        method_ranges(&masked, open, close)
            .into_iter()
            .any(|(fn_start, body_open, body_close)| {
                let signature = code_tokens(&source[fn_start..body_open]);
                if !signature.windows(2).any(|window| window == ["&", "self"])
                    || signature
                        .windows(3)
                        .any(|window| window == ["&", "mut", "self"])
                {
                    return false;
                }
                let body_tokens = code_tokens(&source[body_open + 1..body_close]);
                let statements = top_level_statements(&body_tokens);
                let Some((expression, preceding)) = statements.split_last() else {
                    return false;
                };
                let mut aliases = Vec::new();
                for statement in preceding {
                    let Some((alias, taint)) =
                        aggregate_tainted_alias_from_let(statement, APP_OWNERS, &aliases)
                    else {
                        return false;
                    };
                    aliases.push((alias, taint));
                }
                if !simple_aggregate_forward_expression(expression, APP_OWNERS, &aliases) {
                    return false;
                }
                let expression_taint = aggregate_owner_taint(expression, APP_OWNERS, &aliases);
                expression_taint.is_power_of_two()
                    && GUARDED_OWNERS.iter().any(|owner| {
                        APP_OWNERS
                            .iter()
                            .position(|field| field == owner)
                            .and_then(owner_bit)
                            == Some(expression_taint)
                    })
            })
    })
}

fn aggregate_tainted_alias_from_let<'a>(
    statement: &[&'a str],
    owners: &[&str],
    aliases: &[(&str, u16)],
) -> Option<(&'a str, u16)> {
    if statement.first() != Some(&"let") {
        return None;
    }
    let mut alias_index = 1;
    if statement.get(alias_index) == Some(&"mut") {
        alias_index += 1;
    }
    let alias = *statement.get(alias_index)?;
    if !is_identifier(alias) {
        return None;
    }
    let equals = statement.iter().position(|token| *token == "=")?;
    let expression = &statement[equals + 1..];
    if !simple_aggregate_forward_expression(expression, owners, aliases) {
        return None;
    }
    Some((alias, aggregate_owner_taint(expression, owners, aliases)))
}

fn simple_aggregate_forward_expression(
    tokens: &[&str],
    owners: &[&str],
    aliases: &[(&str, u16)],
) -> bool {
    let mut start = usize::from(tokens.first() == Some(&"return"));
    while matches!(tokens.get(start), Some(&"(" | &"&" | &"mut")) {
        start += 1;
    }
    let rooted_in_owner = (tokens.get(start) == Some(&"self")
        && tokens.get(start + 1) == Some(&".")
        && tokens
            .get(start + 2)
            .is_some_and(|owner| owners.contains(owner)))
        || tokens
            .get(start)
            .is_some_and(|candidate| aliases.iter().any(|(alias, _)| alias == candidate));
    rooted_in_owner
        && tokens.iter().all(|token| {
            is_identifier(token)
                || token.chars().all(|character| character.is_ascii_digit())
                || matches!(*token, "." | "(" | ")" | "," | "&")
        })
}

fn aggregate_owner_taint(tokens: &[&str], owners: &[&str], aliases: &[(&str, u16)]) -> u16 {
    let direct = tokens.windows(3).fold(0_u16, |taint, window| {
        if window[0] != "self" || window[1] != "." {
            return taint;
        }
        owners
            .iter()
            .position(|owner| *owner == window[2])
            .and_then(owner_bit)
            .map_or(taint, |owner| taint | owner)
    });
    tokens.iter().fold(direct, |taint, token| {
        aliases
            .iter()
            .rev()
            .find(|(alias, _)| alias == token)
            .map_or(taint, |(_, owner)| taint | owner)
    })
}

fn owner_bit(index: usize) -> Option<u16> {
    u32::try_from(index)
        .ok()
        .and_then(|shift| 1_u16.checked_shl(shift))
}

fn representation_like_return(source: &str, returned: &str) -> bool {
    let aliases = representation_type_aliases(source);
    type_exposes_representation(&code_tokens(returned), &aliases)
}

fn representation_type_aliases(source: &str) -> Vec<&str> {
    let tokens = code_tokens(source);
    let declarations = type_alias_declarations(&tokens);
    let mut aliases = Vec::new();
    loop {
        let mut added = false;
        for (name, value) in &declarations {
            if !aliases.contains(name) && type_exposes_representation(value, &aliases) {
                aliases.push(*name);
                added = true;
            }
        }
        if !added {
            return aliases;
        }
    }
}

fn type_alias_declarations<'tokens, 'source>(
    tokens: &'tokens [&'source str],
) -> Vec<(&'source str, &'tokens [&'source str])> {
    let mut declarations = Vec::new();
    let mut cursor = 0_usize;
    while cursor < tokens.len() {
        if tokens[cursor] != "type" || tokens.get(cursor.wrapping_sub(1)) == Some(&".") {
            cursor += 1;
            continue;
        }
        let Some(name) = tokens
            .get(cursor + 1)
            .copied()
            .filter(|name| is_identifier(name))
        else {
            cursor += 1;
            continue;
        };
        let end = tokens[cursor + 2..]
            .iter()
            .position(|token| *token == ";")
            .map_or(tokens.len(), |relative| cursor + 2 + relative);
        let Some(equals) = tokens[cursor + 2..end]
            .iter()
            .position(|token| *token == "=")
            .map(|relative| cursor + 2 + relative)
        else {
            cursor = end.saturating_add(1);
            continue;
        };
        declarations.push((name, &tokens[equals + 1..end]));
        cursor = end.saturating_add(1);
    }
    declarations
}

fn type_exposes_representation(tokens: &[&str], aliases: &[&str]) -> bool {
    tokens.iter().any(|token| {
        *token == "&"
            || aliases.contains(token)
            || matches!(
                *token,
                "Task"
                    | "Habit"
                    | "Line"
                    | "TasksRenderState"
                    | "TaskRowsSnapshot"
                    | "TaskTriageSnapshot"
            )
    })
}

fn aggregate_aliases<'a>(tokens: &[&'a str], aggregate: &str) -> Vec<&'a str> {
    let mut aliases = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if *token != "let" {
            continue;
        }
        let end = tokens[index..]
            .iter()
            .position(|candidate| *candidate == ";")
            .map_or(tokens.len(), |relative| index + relative);
        if let Some(alias) = aggregate_alias_from_let(&tokens[index..end], aggregate) {
            aliases.push(alias);
        }
    }
    aliases
}

fn aggregate_alias_from_let<'a>(statement: &[&'a str], aggregate: &str) -> Option<&'a str> {
    if statement.first() != Some(&"let") {
        return None;
    }
    let mut alias_index = 1;
    if statement.get(alias_index) == Some(&"mut") {
        alias_index += 1;
    }
    let alias = *statement.get(alias_index)?;
    if !is_identifier(alias) {
        return None;
    }
    let equals = statement.iter().position(|token| *token == "=")?;
    member_reference(&statement[equals + 1..], aggregate).then_some(alias)
}

fn tainted_alias_from_let<'a>(
    statement: &[&'a str],
    aggregate: &str,
    aliases: &[&str],
) -> Option<&'a str> {
    if statement.first() != Some(&"let") {
        return None;
    }
    let mut alias_index = 1;
    if statement.get(alias_index) == Some(&"mut") {
        alias_index += 1;
    }
    let alias = *statement.get(alias_index)?;
    if !is_identifier(alias) {
        return None;
    }
    let equals = statement.iter().position(|token| *token == "=")?;
    forwarded_expression(&statement[equals + 1..], aggregate, aliases).then_some(alias)
}

fn member_reference(tokens: &[&str], aggregate: &str) -> bool {
    tokens.windows(2).any(|window| window == [".", aggregate])
}

fn top_level_statements<'a>(tokens: &'a [&'a str]) -> Vec<&'a [&'a str]> {
    let mut statements = Vec::new();
    let mut start = 0_usize;
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match *token {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            ";" if depth == 0 => {
                if start < index {
                    statements.push(&tokens[start..index]);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < tokens.len() {
        statements.push(&tokens[start..]);
    }
    statements
}

fn forwarded_expression(tokens: &[&str], aggregate: &str, aliases: &[&str]) -> bool {
    let mut start = 0_usize;
    if tokens.get(start) == Some(&"return") {
        start += 1;
    }
    while matches!(tokens.get(start), Some(&"(" | &"&" | &"mut")) {
        start += 1;
    }
    (tokens.get(start) == Some(&"self")
        && tokens.get(start + 1) == Some(&".")
        && tokens.get(start + 2) == Some(&aggregate))
        || tokens
            .get(start)
            .is_some_and(|candidate| aliases.contains(candidate))
}

fn function_signature<'a>(source: &'a str, name: &str) -> &'a str {
    let masked = mask_non_code(source);
    for (start, _) in masked.match_indices("fn") {
        if !token_at(&masked, start, "fn") {
            continue;
        }
        let after_fn = start + 2;
        let rest = masked[after_fn..].trim_start();
        let name_start = after_fn + (masked[after_fn..].len() - rest.len());
        let Some(after_name) = rest.strip_prefix(name) else {
            continue;
        };
        if !after_name
            .chars()
            .next()
            .is_some_and(|character| character == '(' || character == '<')
        {
            continue;
        }
        let end = masked[name_start + name.len()..]
            .find('{')
            .map(|relative| name_start + name.len() + relative)
            .expect("function body");
        return &source[start..end];
    }
    panic!("function declaration: {name}")
}

fn compact_signature(signature: &str) -> String {
    let mut compact = signature
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    while compact.contains(",)") {
        compact = compact.replace(",)", ")");
    }
    compact
        .strip_prefix("pub(crate)")
        .unwrap_or(&compact)
        .to_owned()
}

fn compact_tokens(source: &str) -> String {
    code_tokens(source).concat()
}

fn has_exact_named_shape(source: &str, kind: &str, name: &str, expected: &str) -> bool {
    extract_named_body(source, kind, name).is_some_and(|body| compact_tokens(body) == expected)
}

fn expected_links_plan_shape() -> String {
    [
        "None,Open",
        "{",
        "url:String",
        "},Choose",
        "{",
        "task_id:String,links:Vec<Link>",
        "},",
    ]
    .concat()
}

fn public_impl_method_names<'a>(source: &'a str, type_name: &str) -> Vec<&'a str> {
    public_impl_method_signatures_with_names(source, type_name)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

fn public_impl_method_signatures(source: &str, type_name: &str) -> Vec<String> {
    public_impl_method_signatures_with_names(source, type_name)
        .into_iter()
        .map(|(_, signature)| compact_signature(signature))
        .collect()
}

fn public_impl_method_signatures_with_names<'a>(
    source: &'a str,
    type_name: &str,
) -> Vec<(&'a str, &'a str)> {
    let masked = mask_non_code(source);
    let mut methods = Vec::new();
    for (impl_open, impl_close) in impl_ranges(&masked, type_name) {
        let mut declaration_start = impl_open + 1;
        for (fn_start, body_open, body_close) in method_ranges(&masked, impl_open, impl_close) {
            let visibility: String = masked[declaration_start..fn_start]
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            declaration_start = body_close + 1;
            if !visibility.contains("pub(crate)") {
                continue;
            }
            let after_fn = &masked[fn_start + 2..body_open];
            let trimmed = after_fn.trim_start();
            let name_len = trimmed
                .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .expect("method name terminator");
            let name_offset = source[fn_start + 2..body_open].find(trimmed).unwrap();
            let name_start = fn_start + 2 + name_offset;
            methods.push((
                &source[name_start..name_start + name_len],
                &source[fn_start..body_open],
            ));
        }
    }
    methods
}

fn public_state_type_names(source: &str) -> Vec<&str> {
    let tokens = code_tokens(source);
    let mut names = Vec::new();
    for window in tokens.windows(6) {
        if window[0] == "pub"
            && window[1] == "("
            && window[2] == "crate"
            && window[3] == ")"
            && matches!(window[4], "struct" | "enum")
        {
            names.push(window[5]);
        }
    }
    names
}

fn code_tokens(source: &str) -> Vec<&str> {
    let masked = mask_non_code(source);
    let bytes = masked.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        } else if bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_' {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len()
                && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
            {
                cursor += 1;
            }
            tokens.push(&source[start..cursor]);
        } else {
            let start = cursor;
            cursor += 1;
            tokens.push(&source[start..cursor]);
        }
    }
    tokens
}

fn declares_function(source: &str, name: &str) -> bool {
    let masked = mask_non_code(source);
    masked.match_indices("fn").any(|(start, _)| {
        if !token_at(&masked, start, "fn") {
            return false;
        }
        let rest = masked[start + 2..].trim_start();
        rest.strip_prefix(name).is_some_and(|after| {
            after
                .chars()
                .next()
                .is_some_and(|character| character == '(' || character == '<')
        })
    })
}

fn impl_app_ranges(masked: &str) -> Vec<(usize, usize)> {
    impl_ranges(masked, "App")
}

fn impl_ranges(masked: &str, type_name: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for (start, _) in masked.match_indices("impl") {
        if !token_at(masked, start, "impl") {
            continue;
        }
        let rest_start = start + 4;
        let rest = masked[rest_start..].trim_start();
        if !rest.starts_with(type_name)
            || rest[type_name.len()..]
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            continue;
        }
        let open = rest_start + (masked[rest_start..].len() - rest.len()) + rest.find('{').unwrap();
        if let Some(close) = matching_brace(masked, open) {
            ranges.push((open, close));
        }
    }
    ranges
}

fn method_ranges(masked: &str, impl_open: usize, impl_close: usize) -> Vec<(usize, usize, usize)> {
    let mut ranges = Vec::new();
    let mut cursor = impl_open + 1;
    let mut depth = 0_usize;
    while cursor < impl_close {
        match masked.as_bytes()[cursor] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'f' if depth == 0 && token_at(masked, cursor, "fn") => {
                let Some(relative_open) = masked[cursor + 2..impl_close].find('{') else {
                    break;
                };
                let open = cursor + 2 + relative_open;
                if let Some(close) = matching_brace(masked, open) {
                    ranges.push((cursor, open, close));
                    cursor = close;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    ranges
}

fn matching_brace(masked: &str, open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (offset, byte) in masked.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn token_at(source: &str, start: usize, token: &str) -> bool {
    source[start..].starts_with(token)
        && source[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
        && source[start + token.len()..]
            .chars()
            .next()
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
}

fn is_identifier(candidate: &str) -> bool {
    let mut characters = candidate.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn mask_non_code(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"//") {
            let end = bytes[cursor..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |length| cursor + length);
            masked[cursor..end].fill(b' ');
            cursor = end;
        } else if bytes[cursor..].starts_with(b"/*") {
            let mut end = cursor + 2;
            let mut depth = 1_usize;
            while end < bytes.len() && depth > 0 {
                if bytes[end..].starts_with(b"/*") {
                    depth += 1;
                    end += 2;
                } else if bytes[end..].starts_with(b"*/") {
                    depth -= 1;
                    end += 2;
                } else {
                    end += 1;
                }
            }
            for byte in &mut masked[cursor..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            cursor = end;
        } else if bytes[cursor] == b'"' {
            let mut end = cursor + 1;
            while end < bytes.len() {
                if bytes[end] == b'\\' {
                    end = (end + 2).min(bytes.len());
                } else if bytes[end] == b'"' {
                    end += 1;
                    break;
                } else {
                    end += 1;
                }
            }
            for byte in &mut masked[cursor..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    String::from_utf8(masked).expect("mask preserves UTF-8 bytes")
}
