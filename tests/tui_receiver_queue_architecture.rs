use std::path::Path;

#[path = "tui_receiver_queue_architecture/ownership.rs"]
mod ownership;

use ownership::{queue_boundary_violations, queue_boundary_violations_at};

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
        "use crate::server::receiver::InboundJob::{self as WorkItem}; struct Runtime { pending: Vec<WorkItem> }",
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

#[test]
fn queue_guard_covers_complete_declared_item_ast() {
    let fixtures = [
        (
            "unicode lifetime and generic default",
            "struct ΔRuntime<'δ, T = InboundJob>(PhantomData<T>);",
        ),
        (
            "where-clause Fn argument",
            "struct Runtime<F>(PhantomData<F>) where F: Fn(InboundJob) -> ();",
        ),
        (
            "const-generic default expression",
            "struct Runtime<const N: usize = { std::mem::size_of::<InboundJob>() }> { bytes: [u8; N] }",
        ),
        (
            "enum generic default",
            "enum Runtime<T = InboundJob> { Empty(PhantomData<T>) }",
        ),
        (
            "union generic default",
            "union Runtime<T: Copy = InboundJob> { marker: std::mem::ManuallyDrop<T> }",
        ),
        (
            "const initializer",
            "const CURRENT: Erased = Erased::of::<InboundJob>();",
        ),
        (
            "associated const initializer",
            "impl Runtime { const CURRENT: Erased = Erased::of::<InboundJob>(); }",
        ),
        (
            "trait associated const type",
            "trait RuntimeState { const CURRENT: Option<InboundJob>; }",
        ),
        (
            "foreign static type",
            "unsafe extern \"C\" { safe static JOBS: Option<InboundJob>; }",
        ),
        (
            "qualified type-alias chain",
            "type Job = crate::server::receiver::InboundJob; type Work = crate::aliases::Job; struct Runtime { current: crate::aliases::Work }",
        ),
        (
            "nested block comment",
            "/* outer /* inner */ \" unclosed for a flat lexer */ struct Runtime<T = InboundJob>(PhantomData<T>);",
        ),
        (
            "raw byte and C literals",
            r####"const RAW: &str = r###"raw " value"###; const BYTES: &[u8] = br##"byte " value"##; const C_TEXT: &CStr = c"C value"; struct Runtime<T = InboundJob>(PhantomData<T>);"####,
        ),
    ];

    assert_rejected_fixtures(&fixtures, "declared persistent item AST");
}

#[test]
fn receiver_effect_exception_requires_exact_path_scope_and_payload_shape() {
    let collision_source = "enum ReceiverEffect { Dispatch(Box<InboundJob>) }";
    assert!(
        !queue_boundary_violations_at(
            Path::new("nested/src/tui/receiver/effect.rs"),
            collision_source,
        )
        .is_empty()
    );

    let nested_source = "mod nested { enum ReceiverEffect { Dispatch(Box<InboundJob>) } }";
    assert!(
        !queue_boundary_violations_at(Path::new("src/tui/receiver/effect.rs"), nested_source,)
            .is_empty()
    );

    let buffered = "enum ReceiverEffect { Dispatch(Box<Vec<InboundJob>>) }";
    assert!(
        !queue_boundary_violations_at(Path::new("src/tui/receiver/effect.rs"), buffered).is_empty()
    );

    let wrong_variant = "enum ReceiverEffect { Other(Box<InboundJob>) }";
    assert!(
        !queue_boundary_violations_at(Path::new("src/tui/receiver/effect.rs"), wrong_variant)
            .is_empty()
    );
}

#[test]
fn queue_guard_rejects_opaque_item_macros_and_custom_attributes() {
    let fixtures = [
        (
            "macro_rules generated storage",
            "macro_rules! declare_storage { ($job:ty) => { struct Runtime { jobs: Vec<$job> } }; } declare_storage!(InboundJob);",
        ),
        (
            "lazy_static storage",
            "lazy_static! { static ref JOBS: Vec<InboundJob> = Vec::new(); }",
        ),
        (
            "opaque item macro invocation",
            "declare_storage!(InboundJob);",
        ),
        (
            "custom persistent-item attribute",
            "#[stores_jobs] struct Runtime { marker: usize }",
        ),
        (
            "custom persistent-item attribute behind cfg_attr",
            "#[cfg_attr(feature = \"jobs\", stores_jobs)] struct Runtime { marker: usize }",
        ),
    ];

    assert_rejected_fixtures(&fixtures, "opaque item macro or attribute");
}

#[test]
fn queue_guard_ignores_test_items_test_sources_and_builtin_attributes() {
    let cfg_test = "#[cfg(test)] mod tests { lazy_static! { static ref JOBS: Vec<InboundJob> = Vec::new(); } }";
    assert!(queue_boundary_violations(cfg_test).is_empty());

    let test_source = "declare_storage!(InboundJob);";
    assert!(
        queue_boundary_violations_at(Path::new("src/tui/tests/generated.rs"), test_source)
            .is_empty()
    );
    assert!(
        queue_boundary_violations_at(Path::new("src/tui/generated_tests.rs"), test_source)
            .is_empty()
    );

    let builtins = r#"
        #[derive(Debug)]
        #[repr(C)]
        #[allow(dead_code)]
        #[doc = "neutral fixture"]
        #[must_use]
        struct Runtime { marker: usize }
    "#;
    assert!(queue_boundary_violations(builtins).is_empty());
}

fn assert_rejected_fixtures(fixtures: &[(&str, &str)], boundary: &str) {
    let mut missed = Vec::new();
    let mut parse_errors = Vec::new();
    for (label, source) in fixtures {
        let violations = queue_boundary_violations(source);
        if violations.is_empty() {
            missed.push(*label);
        }
        if violations
            .iter()
            .any(|violation| violation.starts_with("could not parse production Rust:"))
        {
            parse_errors.push(*label);
        }
    }

    assert!(
        parse_errors.is_empty(),
        "queue guard fixtures must be valid Rust syntax:
{}",
        parse_errors.join("\n")
    );

    assert!(
        missed.is_empty(),
        "queue guard missed {boundary} fixtures:\n{}",
        missed.join("\n")
    );
}
