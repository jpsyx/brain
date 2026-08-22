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
