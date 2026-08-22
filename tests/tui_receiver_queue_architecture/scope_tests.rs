use std::path::Path;

use super::{assert_rejected_fixtures, queue_boundary_violations, queue_boundary_violations_at};

#[test]
fn queue_guard_traverses_every_persistent_item_scope() {
    let fixtures = [
        (
            "function-local static",
            "fn build() { static JOBS: Vec<InboundJob> = Vec::new(); }",
        ),
        (
            "nested-block const",
            "fn build() { if ready() { { const JOB: Option<InboundJob> = None; } } }",
        ),
        (
            "block-local declared types",
            "fn build() { type Jobs = Vec<InboundJob>; struct State { jobs: Jobs } enum Phase { Ready(InboundJob) } union Slot { job: std::mem::ManuallyDrop<InboundJob> } }",
        ),
        (
            "lexically scoped local alias chain",
            "fn build() { use crate::server::receiver::InboundJob as WorkItem; type Job = WorkItem; { type Jobs = Vec<Job>; struct Runtime { pending: Jobs } } }",
        ),
        (
            "impl associated type",
            "impl Runtime { type Jobs = Vec<InboundJob>; }",
        ),
        (
            "trait associated type",
            "trait RuntimeState { type Jobs = Vec<InboundJob>; }",
        ),
        (
            "block item macro",
            "fn build() { macro_rules! declare_storage { () => { static JOBS: Vec<InboundJob> = Vec::new(); } } }",
        ),
        (
            "impl item macro",
            "impl Runtime { declare_storage!(InboundJob); }",
        ),
        (
            "trait item macro",
            "trait RuntimeState { declare_storage!(InboundJob); }",
        ),
        ("opaque foreign type", "unsafe extern \"C\" { type Jobs; }"),
        ("module item verbatim", "const VALUE<T>: usize = 1;"),
        (
            "block item verbatim",
            "fn build() { const VALUE<T>: usize = 1; }",
        ),
        (
            "impl item verbatim",
            "impl Runtime { const VALUE<T>: usize = 1; }",
        ),
        (
            "trait item verbatim",
            "trait RuntimeState { const VALUE<T>: usize; }",
        ),
        (
            "foreign item verbatim",
            "unsafe extern \"C\" { safe static VALUE: usize; }",
        ),
    ];

    assert_rejected_fixtures(&fixtures, "nested persistent item scope");
}

#[test]
fn queue_guard_inspects_macro_tokens_for_resolved_job_aliases() {
    let fixtures = [
        (
            "persistent initializer macro",
            "static JOBS: Erased = store!({ nested(InboundJob) });",
        ),
        (
            "persistent type macro",
            "struct Runtime { jobs: storage_type!([InboundJob]) }",
        ),
        (
            "persistent macro with imported alias",
            "use crate::server::receiver::InboundJob as WorkItem; static JOBS: Erased = store!(((WorkItem)));",
        ),
        (
            "block statement macro",
            "fn build() { observe!({ nested(InboundJob) }); }",
        ),
        (
            "block statement macro with local alias",
            "fn build() { use crate::server::receiver::InboundJob as WorkItem; observe!(((WorkItem))); }",
        ),
    ];

    assert_rejected_fixtures(&fixtures, "macro token job reference");

    let harmless = r"
        fn stage(runtime: &mut ReceiverRuntime, job: InboundJob) {
            let transient: Vec<InboundJob> = Vec::new();
            observe!({ runtime.queue.len() });
            runtime.queue.stage(job);
            drop(transient);
        }
    ";
    assert!(queue_boundary_violations(harmless).is_empty());
}

#[test]
fn receiver_effect_exception_requires_canonical_unshadowable_types() {
    let job = "crate::server::receiver::InboundJob";
    let fixtures = [
        (
            "evil Box path",
            format!("enum ReceiverEffect {{ Dispatch(evil::Box<{job}>) }}"),
        ),
        (
            "local Box alias",
            format!("type Box<T> = Vec<T>; enum ReceiverEffect {{ Dispatch(Box<{job}>) }}"),
        ),
        (
            "generic Box parameter",
            format!("enum ReceiverEffect<Box> {{ Dispatch(Box<{job}>) }}"),
        ),
        (
            "innocuous generic parameter",
            format!(
                "enum ReceiverEffect<T> {{ Dispatch(std::boxed::Box<{job}>), Marker(PhantomData<T>) }}"
            ),
        ),
        (
            "evil RestartPlan path",
            format!(
                "enum ReceiverEffect {{ ApplyRestart(std::boxed::Box<evil::RestartPlan<{job}>>) }}"
            ),
        ),
        (
            "shadowing Box import",
            format!("use evil::Box; enum ReceiverEffect {{ Dispatch(Box<{job}>) }}"),
        ),
        (
            "shadowing RestartPlan import",
            format!(
                "use evil::RestartPlan; enum ReceiverEffect {{ ApplyRestart(std::boxed::Box<RestartPlan<{job}>>) }}"
            ),
        ),
    ];

    let missed = fixtures
        .iter()
        .filter_map(|(label, source)| {
            queue_boundary_violations_at(Path::new("src/tui/receiver/effect.rs"), source)
                .is_empty()
                .then_some(*label)
        })
        .collect::<Vec<_>>();
    assert!(
        missed.is_empty(),
        "receiver effect exception accepted shadowable payloads:\n{}",
        missed.join("\n")
    );

    let canonical = r"
        use evil::{Box, RestartPlan};
        enum ReceiverEffect {
            ApplyRestart(std::boxed::Box<crate::server::receiver::RestartPlan<crate::server::receiver::InboundJob>>),
            ApplyNewSession(std::boxed::Box<crate::server::receiver::InboundJob>),
            Dispatch(std::boxed::Box<crate::server::receiver::InboundJob>),
        }
    ";
    assert!(
        queue_boundary_violations_at(Path::new("src/tui/receiver/effect.rs"), canonical).is_empty()
    );
}

#[test]
fn cfg_implication_ignores_only_test_only_items_and_attributes() {
    let ignored = [
        "#[cfg(test)] struct Runtime { jobs: Vec<InboundJob> }",
        "#[cfg(all(test, unix))] struct Runtime { jobs: Vec<InboundJob> }",
        "#[cfg(any(test, all(test, unix)))] struct Runtime { jobs: Vec<InboundJob> }",
        "#[cfg_attr(test, stores_jobs)] struct Runtime { marker: usize }",
        "#[cfg_attr(all(test, unix), stores_jobs)] struct Runtime { marker: usize }",
    ];
    assert!(
        ignored
            .into_iter()
            .all(|source| queue_boundary_violations(source).is_empty())
    );

    let production = [
        "#[cfg(any(test, feature = \"jobs\"))] struct Runtime { jobs: Vec<InboundJob> }",
        "#[cfg(not(not(test)))] struct Runtime { jobs: Vec<InboundJob> }",
        "#[cfg_attr(any(test, feature = \"jobs\"), stores_jobs)] struct Runtime { marker: usize }",
    ];
    assert!(
        production
            .into_iter()
            .all(|source| !queue_boundary_violations(source).is_empty())
    );
}

#[test]
fn queue_guard_canonicalizes_raw_identifiers_at_every_alias_boundary() {
    let fixtures = [
        (
            "raw direct job path",
            "struct Runtime { current: r#InboundJob }",
        ),
        (
            "raw type alias identity",
            "type r#WorkItem = r#InboundJob; struct Runtime { current: r#WorkItem }",
        ),
        (
            "raw direct import alias",
            "use crate::server::receiver::r#InboundJob as r#WorkItem; struct Runtime { current: r#WorkItem }",
        ),
        (
            "raw grouped import alias",
            "use crate::server::receiver::{r#InboundJob as r#WorkItem}; struct Runtime { current: r#WorkItem }",
        ),
        (
            "raw macro token",
            "static JOB: Erased = store!(r#InboundJob);",
        ),
    ];

    assert_rejected_fixtures(&fixtures, "raw identifier aliases");

    let effect = r"
        enum r#ReceiverEffect {
            r#Dispatch(std::boxed::Box<crate::server::receiver::r#InboundJob>),
        }
    ";
    assert!(
        queue_boundary_violations_at(Path::new("src/tui/receiver/effect.rs"), effect).is_empty(),
        "raw spellings must preserve the exact one-shot receiver-effect exception"
    );
}

#[test]
fn visible_renamed_job_reexports_are_rejected_at_the_declaring_module() {
    let api = "pub(crate) use crate::server::receiver::InboundJob as WorkItem;";
    let owner = "use crate::api::WorkItem; struct Runtime { current: WorkItem }";

    assert!(
        !queue_boundary_violations_at(Path::new("src/tui/api.rs"), api).is_empty(),
        "the public alias declaration must violate even though a sibling import cannot recover its origin"
    );
    assert!(
        queue_boundary_violations_at(Path::new("src/tui/owner.rs"), owner).is_empty(),
        "the enforceable invariant belongs to the visible re-export declaration"
    );

    for visibility in ["pub", "pub(crate)", "pub(super)", "pub(in crate::tui)"] {
        let source =
            format!("{visibility} use crate::server::receiver::r#InboundJob as r#WorkItem;");
        assert!(
            !queue_boundary_violations_at(Path::new("src/tui/api.rs"), &source).is_empty(),
            "{visibility} raw rename must be rejected"
        );
    }

    let private =
        "use crate::server::receiver::InboundJob as WorkItem; struct Runtime { current: WorkItem }";
    assert!(
        !queue_boundary_violations_at(Path::new("src/tui/private.rs"), private).is_empty(),
        "private same-scope aliases remain resolved through their persistent use"
    );
}
