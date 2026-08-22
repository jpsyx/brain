use std::path::Path;

#[path = "tui_receiver_queue_architecture/ownership.rs"]
mod ownership;
#[path = "tui_receiver_queue_architecture/tokens.rs"]
mod tokens;

use ownership::{queue_boundary_violations, queue_boundary_violations_at};
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
fn queue_guard_resolves_direct_and_grouped_import_aliases() {
    let fixtures = [
        "use crate::server::receiver::InboundJob as WorkItem; struct Runtime { pending: Vec<WorkItem> }",
        "use crate::server::receiver::{InboundJob as WorkItem}; struct Runtime { pending: Vec<WorkItem> }",
    ];
    let missed = fixtures
        .into_iter()
        .filter(|source| queue_boundary_violations(source).is_empty())
        .collect::<Vec<_>>();

    assert!(
        missed.is_empty(),
        "queue guard missed imported InboundJob aliases:\n{}",
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
fn queue_guard_rejects_persistent_const_and_static_job_owners() {
    let fixtures = [
        "static JOBS: std::sync::OnceLock<Vec<InboundJob>> = std::sync::OnceLock::new();",
        "pub(crate) static JOBS: OnceLock<std::collections::VecDeque<crate::server::receiver::InboundJob>> = OnceLock::new();",
        "const CURRENT: Option<InboundJob> = None;",
        "pub const CURRENT: Holder<crate::server::receiver::InboundJob> = Holder::empty();",
    ];
    let missed = fixtures
        .into_iter()
        .filter(|source| queue_boundary_violations(source).is_empty())
        .collect::<Vec<_>>();

    assert!(
        missed.is_empty(),
        "queue guard missed persistent const/static owners:\n{}",
        missed.join("\n")
    );
}

#[test]
fn queue_guard_ignores_comments_literals_and_owned_api_calls() {
    let source = r#"
        use crate::server::receiver::InboundJob as WorkItem;
        // VecDeque<InboundJob> and runtime.jobs.push_back(job)
        const EXAMPLE: &str = "runtime.queue.pop_front()";
        fn stage(runtime: &mut ReceiverRuntime, job: WorkItem) {
            let _ = EXAMPLE;
            let transient: Vec<WorkItem> = Vec::new();
            drop(transient);
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
fn queue_guard_does_not_let_raw_strings_hide_persistent_storage() {
    let fixtures = [
        r##"const NOTE: &str = r#"raw " delimiter"#; struct Runtime { jobs: Vec<InboundJob> }"##,
        r####"const NOTE: &str = r###"raw "# and "## delimiters"###; struct Runtime { jobs: Vec<InboundJob> }"####,
    ];

    assert!(
        fixtures
            .into_iter()
            .all(|source| !queue_boundary_violations(source).is_empty())
    );
}

#[test]
fn receiver_effect_exception_is_scoped_to_its_real_owner() {
    let source = "enum ReceiverEffect { Dispatch(Box<InboundJob>) }";

    assert!(!queue_boundary_violations(source).is_empty());
}
