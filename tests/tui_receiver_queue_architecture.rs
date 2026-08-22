use std::path::Path;

#[test]
fn inbound_queue_representation_and_mutation_stay_inside_queue_module() {
    let tui_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let queue_path = tui_root.join("receiver/queue.rs");
    let mut leaks = Vec::new();

    for entry in walkdir::WalkDir::new(&tui_root) {
        let entry = entry.expect("walk TUI source");
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || path == queue_path
        {
            continue;
        }

        let source = std::fs::read_to_string(path).expect("read TUI source");
        for violation in queue_boundary_violations(&source) {
            leaks.push(format!("{}: {violation}", path.display()));
        }
    }

    assert!(
        leaks.is_empty(),
        "inbound queue representation or mutation leaked outside receiver/queue.rs:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn queue_guard_recognizes_qualified_types_spacing_and_renamed_aliases() {
    let fixtures = [
        "struct Runtime { jobs: std::vec::Vec < crate::server::receiver::InboundJob > }",
        "struct Runtime { jobs: std::collections::VecDeque < crate::server::receiver::InboundJob > }",
        "fn leak(runtime: &mut Runtime, job: InboundJob) { let pending = &mut runtime.queue; pending.push(job); }",
        "fn leak(runtime: &mut Runtime) { runtime.queue . remove ( 0 ); }",
        "fn leak(runtime: &mut Runtime, job: InboundJob) { runtime.jobs.push_back(job); }",
        "fn leak(runtime: &mut Runtime) { let pending = &mut runtime.jobs; pending.pop_front(); }",
        "fn leak(runtime: &mut Runtime) { runtime.receiver_queue.pop_back(); }",
        "fn leak(runtime: &mut Runtime) { runtime.jobs.drain(..); }",
        "fn leak(runtime: &mut Runtime) { let pending = &mut runtime.receiver_queue; pending.split_off(1); }",
    ];
    let missed = fixtures
        .into_iter()
        .filter(|source| queue_boundary_violations(source).is_empty())
        .collect::<Vec<_>>();

    assert!(
        missed.is_empty(),
        "queue guard missed realistic representation fixtures:\n{}",
        missed.join("\n")
    );
}

#[test]
fn queue_guard_ignores_comments_literals_and_owned_api_calls() {
    let source = r#"
        // VecDeque<InboundJob> and runtime.jobs.push_back(job)
        const EXAMPLE: &str = "runtime.queue.pop_front()";
        fn stage(runtime: &mut ReceiverRuntime, job: InboundJob) {
            let _ = EXAMPLE;
            runtime.queue.stage(job);
        }
    "#;

    assert!(queue_boundary_violations(source).is_empty());
}

fn queue_boundary_violations(source: &str) -> Vec<&'static str> {
    let tokens = rust_tokens(source);
    let mut violations = Vec::new();
    if tokens.iter().enumerate().any(|(index, token)| {
        matches!(token.as_str(), "Vec" | "VecDeque")
            && tokens.get(index + 1).is_some_and(|token| token == "<")
            && tokens[index + 2..]
                .iter()
                .take_while(|token| token.as_str() != ">")
                .any(|token| token == "InboundJob")
    }) {
        violations.push("queue collection containing InboundJob");
    }

    let mut aliases = vec![
        "jobs".to_owned(),
        "queue".to_owned(),
        "receiver_queue".to_owned(),
    ];
    for (let_index, _) in tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| *token == "let")
    {
        let end = tokens[let_index..]
            .iter()
            .position(|token| token == ";")
            .map_or(tokens.len(), |relative| let_index + relative);
        let statement = &tokens[let_index..end];
        if let Some(equals) = statement.iter().position(|token| token == "=")
            && statement[equals + 1..].windows(2).any(|window| {
                window[0] == "."
                    && matches!(window[1].as_str(), "jobs" | "queue" | "receiver_queue")
            })
            && let Some(alias) =
                statement.get(usize::from(statement.get(1).is_some_and(|token| token == "mut")) + 1)
        {
            aliases.push(alias.clone());
        }
    }
    if tokens.windows(3).any(|window| {
        aliases.contains(&window[0])
            && ((window[1] == "."
                && matches!(
                    window[2].as_str(),
                    "push"
                        | "pop"
                        | "remove"
                        | "split_off"
                        | "push_back"
                        | "pop_front"
                        | "pop_back"
                        | "drain"
                ))
                || window[1] == "[")
    }) {
        violations.push("queue representation mutation");
    }
    violations
}

fn rust_tokens(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0_usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"//") {
            index = source[index..]
                .find('\n')
                .map_or(bytes.len(), |relative| index + relative + 1);
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index = bytes[index + 2..]
                .windows(2)
                .position(|window| window == b"*/")
                .map_or(bytes.len(), |relative| index + relative + 4);
            continue;
        }
        if bytes[index] == b'"' || bytes[index] == b'\'' {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        let start = index;
        if bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' {
            index += 1;
            while bytes
                .get(index)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                index += 1;
            }
        } else if bytes[index..].starts_with(b"::") {
            index += 2;
        } else {
            index += 1;
        }
        tokens.push(source[start..index].to_owned());
    }
    tokens
}
