use std::collections::HashSet;
use std::path::Path;

#[path = "tui_receiver_queue_architecture/tokens.rs"]
mod tokens;

use tokens::rust_tokens;

#[test]
fn raw_inbound_job_storage_stays_inside_queue_module() {
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
        for violation in queue_boundary_violations_at(path, &source) {
            leaks.push(format!("{}: {violation}", path.display()));
        }
    }

    assert!(
        leaks.is_empty(),
        "persistent raw InboundJob storage leaked outside receiver/queue.rs:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn queue_guard_is_independent_of_field_collection_and_mutator_names() {
    let fixtures = [
        "struct Runtime { jobs: std::vec::Vec < crate::server::receiver::InboundJob > }",
        "struct Runtime { jobs: std::collections::VecDeque < crate::server::receiver::InboundJob > }",
        "struct Runtime { pending_work: Ring<InboundJob> } fn leak(runtime: &mut Runtime, job: InboundJob) { runtime.pending_work.absorb(job); }",
        "struct Runtime { envelopes: Store<InboundJob> } fn leak(runtime: &mut Runtime) { let alias = &mut runtime.envelopes; alias.rotate_anyhow(); }",
        "type Backlog = vendor::NovelStore<InboundJob>; struct Runtime { arbitrary: Backlog }",
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
fn queue_guard_rejects_any_second_persistent_inbound_job_owner() {
    let fixtures = [
        "struct Runtime { current: InboundJob }",
        "struct Runtime { current: Box<InboundJob> }",
        "struct Runtime { current: Option<InboundJob> }",
        "struct Runtime { pending_work: std::collections::LinkedList<InboundJob> }",
        "struct Runtime { backlog: std::collections::BTreeMap<u64, InboundJob> }",
        "struct Runtime(std::collections::BinaryHeap<InboundJob>);",
        "enum Runtime { Waiting(std::collections::LinkedList<InboundJob>) }",
        "enum ReceiverEffect { Raw(InboundJob) }",
        "enum ReceiverEffect { Buffered(Box<Vec<InboundJob>>) }",
        "enum ReceiverEffect { Buffered(Box<[InboundJob; 4]>) }",
        "enum ReceiverEffect { Buffered(Box<(InboundJob, InboundJob)>) }",
        "type Backlog = std::collections::LinkedList<InboundJob>; struct Runtime { pending: Backlog }",
        "type Job = InboundJob; type Backlog = Vec<Job>; struct Runtime { pending: Backlog }",
    ];
    let missed = fixtures
        .into_iter()
        .filter(|source| queue_boundary_violations(source).is_empty())
        .collect::<Vec<_>>();

    assert!(
        missed.is_empty(),
        "queue guard missed persistent InboundJob owners:\n{}",
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

#[test]
fn queue_guard_allows_only_typed_one_shot_receiver_effect_payloads() {
    let source = r"
        enum ReceiverEffect {
            ApplyRestart(Box<RestartPlan<InboundJob>>),
            ApplyNewSession(Box<InboundJob>),
            Dispatch(Box<InboundJob>),
        }
    ";

    assert!(
        queue_boundary_violations_at(Path::new("src/tui/receiver/effect.rs"), source).is_empty()
    );
}

#[test]
fn queue_guard_does_not_let_lifetimes_hide_persistent_storage() {
    let source = "struct Runtime<'a> { pending: Vec<InboundJob>, marker: PhantomData<&'a ()> }";

    assert!(!queue_boundary_violations(source).is_empty());
    assert_eq!(rust_tokens("'a'"), Vec::<String>::new());
    assert!(
        rust_tokens("&'a ()")
            .windows(2)
            .any(|tokens| tokens == ["'", "a"])
    );
}

#[test]
fn receiver_effect_exception_is_scoped_to_its_real_owner() {
    let source = "enum ReceiverEffect { Dispatch(Box<InboundJob>) }";

    assert!(!queue_boundary_violations(source).is_empty());
}

fn queue_boundary_violations(source: &str) -> Vec<&'static str> {
    queue_boundary_violations_at(Path::new("src/tui/unrelated.rs"), source)
}

fn queue_boundary_violations_at(path: &Path, source: &str) -> Vec<&'static str> {
    let tokens = rust_tokens(source);
    let owns_receiver_effect_boundary = path.ends_with("src/tui/receiver/effect.rs");
    if declares_raw_inbound_job_storage(&tokens, owns_receiver_effect_boundary) {
        vec!["persistent type owns raw InboundJob storage"]
    } else {
        Vec::new()
    }
}

fn declares_raw_inbound_job_storage(
    tokens: &[String],
    owns_receiver_effect_boundary: bool,
) -> bool {
    let (job_aliases, storage_aliases) = classify_job_aliases(tokens);
    if job_aliases.len() > 1 || !storage_aliases.is_empty() {
        return true;
    }

    tokens.iter().enumerate().any(|(index, token)| {
        if !matches!(token.as_str(), "struct" | "enum" | "union") {
            return false;
        }
        let Some(open) = tokens[index + 2..]
            .iter()
            .position(|candidate| matches!(candidate.as_str(), "{" | "(" | ";"))
            .map(|relative| index + 2 + relative)
        else {
            return false;
        };
        if tokens[open] == ";" {
            return false;
        }
        let closing = if tokens[open] == "{" { "}" } else { ")" };
        matching_index(tokens, open, &tokens[open], closing).is_some_and(|close| {
            let body = &tokens[open + 1..close];
            if !contains_job_reference(body, &job_aliases, &storage_aliases) {
                return false;
            }
            let declaration_name = tokens.get(index + 1).map(String::as_str);
            !(owns_receiver_effect_boundary
                && declaration_name == Some("ReceiverEffect")
                && receiver_effect_payloads_are_one_shot(body, &job_aliases))
        })
    })
}

fn receiver_effect_payloads_are_one_shot(tokens: &[String], job_aliases: &HashSet<String>) -> bool {
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| job_aliases.contains(*token))
        .all(|(job_index, _)| {
            let wrappers = tokens
                .iter()
                .enumerate()
                .filter_map(|(open, token)| {
                    if token != "<" {
                        return None;
                    }
                    let close = matching_index(tokens, open, "<", ">")?;
                    (open < job_index && job_index < close).then(|| {
                        tokens[..open]
                            .iter()
                            .rev()
                            .find(|candidate| is_identifier(candidate))
                            .map(String::as_str)
                    })
                })
                .collect::<Option<Vec<_>>>();
            if !matches!(wrappers.as_deref(), Some(["Box"] | ["Box", "RestartPlan"])) {
                return false;
            }
            let Some((box_open, box_close)) =
                tokens.iter().enumerate().find_map(|(open, token)| {
                    if token != "<" {
                        return None;
                    }
                    let close = matching_index(tokens, open, "<", ">")?;
                    let outer = tokens[..open]
                        .iter()
                        .rev()
                        .find(|candidate| is_identifier(candidate))?;
                    (outer == "Box" && open < job_index && job_index < close)
                        .then_some((open, close))
                })
            else {
                return false;
            };
            let payload = &tokens[box_open + 1..box_close];
            !payload
                .iter()
                .any(|token| matches!(token.as_str(), "[" | "]" | "(" | ")" | "," | ";"))
                && payload
                    .iter()
                    .filter(|token| job_aliases.contains(*token))
                    .count()
                    == 1
        })
}

fn classify_job_aliases(tokens: &[String]) -> (HashSet<String>, HashSet<String>) {
    let mut job_aliases = HashSet::from(["InboundJob".to_owned()]);
    let mut storage_aliases = HashSet::new();
    loop {
        let mut changed = false;
        for (index, token) in tokens.iter().enumerate() {
            if token != "type" {
                continue;
            }
            let Some(name) = tokens.get(index + 1) else {
                continue;
            };
            let Some(equals) = tokens[index + 2..]
                .iter()
                .position(|candidate| candidate == "=")
                .map(|relative| index + 2 + relative)
            else {
                continue;
            };
            let end = tokens[equals + 1..]
                .iter()
                .position(|candidate| candidate == ";")
                .map_or(tokens.len(), |relative| equals + 1 + relative);
            let target = &tokens[equals + 1..end];
            if contains_raw_job_storage(target, &job_aliases, &storage_aliases) {
                changed |= storage_aliases.insert(name.clone());
            } else if target
                .iter()
                .any(|candidate| job_aliases.contains(candidate))
            {
                changed |= job_aliases.insert(name.clone());
            }
        }
        if !changed {
            return (job_aliases, storage_aliases);
        }
    }
}

fn contains_raw_job_storage(
    tokens: &[String],
    job_aliases: &HashSet<String>,
    storage_aliases: &HashSet<String>,
) -> bool {
    if tokens.iter().any(|token| storage_aliases.contains(token)) {
        return true;
    }

    for (open, token) in tokens.iter().enumerate() {
        if token == "["
            && matching_index(tokens, open, "[", "]").is_some_and(|close| {
                contains_job_reference(&tokens[open + 1..close], job_aliases, storage_aliases)
            })
        {
            return true;
        }
        if token != "<" {
            continue;
        }
        let Some(close) = matching_index(tokens, open, "<", ">") else {
            continue;
        };
        let arguments = &tokens[open + 1..close];
        if !contains_job_reference(arguments, job_aliases, storage_aliases) {
            continue;
        }
        let outer = tokens[..open]
            .iter()
            .rev()
            .find(|candidate| is_identifier(candidate))
            .map(String::as_str);
        if !matches!(outer, Some("Box" | "Option" | "RestartPlan"))
            || contains_raw_job_storage(arguments, job_aliases, storage_aliases)
        {
            return true;
        }
    }
    false
}

fn contains_job_reference(
    tokens: &[String],
    job_aliases: &HashSet<String>,
    storage_aliases: &HashSet<String>,
) -> bool {
    tokens
        .iter()
        .any(|token| job_aliases.contains(token) || storage_aliases.contains(token))
}

fn matching_index(tokens: &[String], open: usize, opening: &str, closing: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token == opening {
            depth += 1;
        } else if token == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn is_identifier(token: &str) -> bool {
    token
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}
