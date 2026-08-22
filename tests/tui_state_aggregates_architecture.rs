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
    let app_body = extract_struct_body(&app, "App").expect("App body");
    let tasks_body = extract_struct_body(&tasks, "TasksState").expect("TasksState body");
    let shell_body = extract_struct_body(&shell, "ShellState").expect("ShellState body");

    assert_eq!(field_declaration_count(app_body, "tasks"), 1);
    assert_eq!(field_declaration_count(app_body, "shell"), 1);
    assert!(field_is_private(app_body, "tasks"));
    assert!(field_is_private(app_body, "shell"));

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
        if has_simple_aggregate_forwarder(&source) {
            leaks.push(format!(
                "{}: simple App aggregate forwarder",
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
    assert!(
        !function_signature(&logs, "handle_logs_key").contains("App"),
        "log handling must accept ShellState"
    );
    assert!(
        !function_signature(&task_handler, "handle_search_key").contains("App"),
        "task-search handling must accept TasksState"
    );
    assert!(
        !function_signature(&brain_renderer, "draw_brain").contains("App"),
        "brain rendering must accept a focused projection"
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

    let duplicate_visibility = r"
        struct App {
            pub(super) query: String,
            pub(in crate::tui) query: String,
        }
    ";
    let body = extract_struct_body(duplicate_visibility, "App").expect("App body");
    assert_eq!(field_declaration_count(body, "query"), 2);

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

    let forwarding = r"
        impl App {
            fn query(&self) -> &str { self.tasks.query_text() }
        }
    ";
    assert!(has_simple_aggregate_forwarder(forwarding));
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
    let masked = mask_non_code(source);
    let mut offset = 0;
    while let Some(relative) = masked[offset..].find("struct") {
        let start = offset + relative;
        if !token_at(&masked, start, "struct") {
            offset = start + "struct".len();
            continue;
        }
        let mut cursor = start + "struct".len();
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
    let masked = mask_non_code(source);
    let mut aliases = Vec::new();
    for line in masked.lines() {
        let compact: String = line
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        let Some(rest) = compact.strip_prefix("let") else {
            continue;
        };
        let Some((left, right)) = rest.split_once('=') else {
            continue;
        };
        let alias = left.strip_prefix("mut").unwrap_or(left);
        if !is_identifier(alias) {
            continue;
        }
        let aggregate_ref = format!(".{aggregate}");
        if right.contains(&aggregate_ref) && !right.contains(&format!(".{aggregate}.")) {
            aliases.push(alias.to_owned());
        }
    }
    let compact: String = masked
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    aliases.iter().any(|alias| {
        fields.iter().any(|field| {
            let needle = format!("{alias}.{field}");
            compact.match_indices(&needle).any(|(at, _)| {
                compact[at + needle.len()..]
                    .chars()
                    .next()
                    .is_some_and(|next| next != '(' && !next.is_ascii_alphanumeric() && next != '_')
            })
        })
    })
}

fn has_simple_aggregate_forwarder(source: &str) -> bool {
    let masked = mask_non_code(source);
    impl_app_ranges(&masked).into_iter().any(|(open, close)| {
        method_body_ranges(&masked, open, close)
            .into_iter()
            .any(|(body_open, body_close)| {
                let mut body: String = masked[body_open + 1..body_close]
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect();
                if let Some(returned) = body.strip_prefix("return") {
                    body = returned.to_owned();
                }
                if body.ends_with(';') {
                    body.pop();
                }
                if body.contains(';') || body.matches("self.").count() != 1 {
                    return false;
                }
                let expression = body
                    .strip_prefix("&mut")
                    .or_else(|| body.strip_prefix('&'))
                    .unwrap_or(&body);
                expression.starts_with("self.tasks") || expression.starts_with("self.shell")
            })
    })
}

fn function_signature<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source
        .find(&format!("fn {name}"))
        .expect("function declaration");
    let rest = &source[start..];
    let end = rest.find('{').expect("function body");
    &rest[..end]
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
    let mut ranges = Vec::new();
    for (start, _) in masked.match_indices("impl") {
        if !token_at(masked, start, "impl") {
            continue;
        }
        let rest_start = start + 4;
        let rest = masked[rest_start..].trim_start();
        if !rest.starts_with("App")
            || rest[3..]
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

fn method_body_ranges(masked: &str, impl_open: usize, impl_close: usize) -> Vec<(usize, usize)> {
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
                    ranges.push((open, close));
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
