use std::path::{Path, PathBuf};

#[path = "tui_receiver_dispatch_architecture/analysis.rs"]
mod analysis;
#[path = "tui_receiver_dispatch_architecture/round_eight_mutations.rs"]
mod round_eight_mutations;
#[path = "tui_receiver_dispatch_architecture/round_eleven_mutations.rs"]
mod round_eleven_mutations;
#[path = "tui_receiver_dispatch_architecture/round_fifteen_mutations.rs"]
mod round_fifteen_mutations;
#[path = "tui_receiver_dispatch_architecture/round_five_mutations.rs"]
mod round_five_mutations;
#[path = "tui_receiver_dispatch_architecture/round_four_mutations.rs"]
mod round_four_mutations;
#[path = "tui_receiver_dispatch_architecture/round_fourteen_mutations.rs"]
mod round_fourteen_mutations;
#[path = "tui_receiver_dispatch_architecture/round_nine_mutations.rs"]
mod round_nine_mutations;
#[path = "tui_receiver_dispatch_architecture/round_seven_mutations.rs"]
mod round_seven_mutations;
#[path = "tui_receiver_dispatch_architecture/round_six_mutations.rs"]
mod round_six_mutations;
#[path = "tui_receiver_dispatch_architecture/round_ten_mutations.rs"]
mod round_ten_mutations;
#[path = "tui_receiver_dispatch_architecture/round_thirteen_mutations.rs"]
mod round_thirteen_mutations;
#[path = "tui_receiver_dispatch_architecture/round_three_mutations.rs"]
mod round_three_mutations;
#[path = "tui_receiver_dispatch_architecture/round_twelve_mutations.rs"]
mod round_twelve_mutations;
#[path = "tui_receiver_dispatch_architecture/round_two_mutations.rs"]
mod round_two_mutations;
#[path = "tui_receiver_dispatch_architecture/source.rs"]
mod source;

fn production_sources(root: &Path) -> Vec<(PathBuf, String)> {
    source::production_source_paths(root)
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            (path, source)
        })
        .collect()
}

#[test]
fn production_receiver_dispatch_uses_only_isolated_durable_runs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let facade_path = root.join("src/tui/app_brain/receiver/mod.rs");
    let facade = std::fs::read_to_string(&facade_path).expect("read receiver facade");
    let facade = syn::parse_file(&facade)
        .unwrap_or_else(|error| panic!("parse {}: {error}", facade_path.display()));
    assert!(
        facade.items.iter().any(|item| {
            matches!(item, syn::Item::Mod(module) if module.ident == "control" && !source::is_exact_cfg_test(&module.attrs))
        }),
        "durable receiver controls must compile with the live consumer"
    );
    let runtime_facade_path = root.join("src/tui/receiver/mod.rs");
    let runtime_facade =
        std::fs::read_to_string(&runtime_facade_path).expect("read receiver runtime facade");
    let runtime_facade = syn::parse_file(&runtime_facade)
        .unwrap_or_else(|error| panic!("parse {}: {error}", runtime_facade_path.display()));
    let runtime_module = runtime_facade
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Mod(module) if module.ident == "runtime" => Some(module),
            _ => None,
        })
        .expect("live runtime module declaration");
    assert!(
        !runtime_module.attrs.iter().any(has_dead_code_allowance),
        "live durable runtime APIs must not hide behind a module-wide dead-code allowance"
    );
    let runtime_path = root.join("src/tui/receiver/runtime.rs");
    let runtime = std::fs::read_to_string(&runtime_path).expect("read receiver runtime");
    let runtime = syn::parse_file(&runtime)
        .unwrap_or_else(|error| panic!("parse {}: {error}", runtime_path.display()));
    for operation in [
        "take_durable_run",
        "store_durable_run",
        "is_enabled",
        "record_intent",
    ] {
        let method = runtime
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Impl(item_impl) => Some(&item_impl.items),
                _ => None,
            })
            .flatten()
            .find_map(|item| match item {
                syn::ImplItem::Fn(method) if method.sig.ident == operation => Some(method),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing live durable runtime operation {operation}"));
        assert!(
            !method.attrs.iter().any(has_dead_code_allowance),
            "live durable runtime operation {operation} must remain linted"
        );
    }
    for current in [
        "active.rs",
        "artifact.rs",
        "control.rs",
        "dispatch.rs",
        "reply.rs",
    ] {
        let path = root.join("src/tui/app_brain/receiver").join(current);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {current}: {error}"));
        let syntax = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        let mut allowance = DeadCodeAllowance::default();
        syn::visit::Visit::visit_file(&mut allowance, &syntax);
        assert!(
            !allowance.found,
            "live receiver module {current} must remain fully linted"
        );
    }
}

#[test]
fn event_loop_has_only_one_receiver_consumer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let consumers = analysis::receiver_tick_call_count(root);

    assert_eq!(
        consumers, 1,
        "receiver dispatch must have one live consumer"
    );
}

#[test]
fn receiver_source_cannot_reach_interactive_execution_or_activity_waits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let violations = analysis::receiver_violations(root);

    assert!(
        violations.is_empty(),
        "receiver-facing production source reaches the interactive panel or waits on its activity:\n{}",
        violations.join("\n")
    );
}

#[test]
fn receiver_source_has_no_socket_or_in_memory_consumer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let violations = analysis::receiver_violations(root);

    assert!(
        violations.is_empty(),
        "production source retains a socket or in-memory receiver consumer:\n{}",
        violations.join("\n")
    );
}

#[test]
fn production_ownership_follows_cfg_test_module_graph_not_filenames() {
    let fixture = tempfile::tempdir().expect("create module-ownership fixture");
    let src = fixture.path().join("src");
    std::fs::create_dir_all(src.join("ordinary/tests")).expect("create fixture modules");
    std::fs::write(
        src.join("lib.rs"),
        "mod ordinary;\nmod receiver_tests;\nmod test_support;\n#[cfg(test)] mod hidden;\n#[cfg(test)] include!(\"included_test.rs\");\n",
    )
    .expect("write fixture root");
    std::fs::write(src.join("ordinary.rs"), "mod tests;\n").expect("write ordinary fixture");
    std::fs::write(src.join("ordinary/tests/mod.rs"), "pub fn live() {}\n")
        .expect("write misleading tests directory fixture");
    std::fs::write(src.join("receiver_tests.rs"), "pub fn live() {}\n")
        .expect("write misleading tests filename fixture");
    std::fs::write(src.join("test_support.rs"), "pub fn live() {}\n")
        .expect("write misleading support filename fixture");
    std::fs::write(src.join("hidden.rs"), "pub fn test_only() {}\n")
        .expect("write exact cfg test fixture");
    std::fs::write(
        src.join("included_test.rs"),
        "pub fn included_test_only() {}\n",
    )
    .expect("write exact cfg test include fixture");

    let paths = production_sources(fixture.path())
        .into_iter()
        .map(|(path, _)| path.strip_prefix(fixture.path()).unwrap().to_owned())
        .collect::<Vec<_>>();

    for production in [
        "src/ordinary/tests/mod.rs",
        "src/receiver_tests.rs",
        "src/test_support.rs",
    ] {
        assert!(
            paths.contains(&PathBuf::from(production)),
            "unconditional module with a misleading test name is production: {production}"
        );
    }
    assert!(
        !paths.contains(&PathBuf::from("src/hidden.rs")),
        "a module reached only through exact cfg(test) is test-owned"
    );
    assert!(
        !paths.contains(&PathBuf::from("src/included_test.rs")),
        "an include reached only through exact cfg(test) is test-owned"
    );
}

#[test]
fn production_graph_preserves_declared_module_identity_for_reachability() {
    let fixture = rust_fixture(&[
        (
            "lib.rs",
            "mod neutral;\n#[path = \"test_support.rs\"] mod receiver;\n",
        ),
        (
            "test_support.rs",
            "pub fn dispatch(controller: &mut crate::agent::controller::AgentController) { crate::neutral::drive(controller); }\n",
        ),
        (
            "neutral.rs",
            "pub fn drive(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n",
        ),
    ]);

    assert!(
        !fixture_receiver_violations(fixture.path()).is_empty(),
        "reachability must use the declared module identity, not the module file name"
    );
}

#[test]
fn undeclared_receiver_orphans_default_to_production() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod interactive;\n"),
        (
            "interactive.rs",
            "pub fn submit(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n",
        ),
        (
            "orphan_receiver.rs",
            "pub fn dispatch(controller: &mut crate::agent::controller::AgentController) { crate::interactive::submit(controller); }\n",
        ),
    ]);

    assert!(
        !fixture_receiver_violations(fixture.path()).is_empty(),
        "an undeclared receiver orphan remains production and reaches neutral helpers"
    );
}

#[test]
fn receiver_reachability_follows_calls_into_neutral_helpers() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "receiver.rs",
            "pub fn dispatch(controller: &mut crate::agent::controller::AgentController) { crate::neutral::drive(controller); }\n",
        ),
        (
            "neutral.rs",
            "pub fn drive(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); controller.type_text(\"remote\"); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.len() >= 2,
        "a neutral helper called by receiver execution cannot submit or type into the interactive controller: {violations:?}"
    );
}

#[test]
fn receiver_reachability_resolves_ufcs_and_type_aliases() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "use crate::agent::controller::AgentController as Frontend;\npub fn dispatch(controller: &mut Frontend) { Frontend::type_text(controller, \"remote\"); }\n",
        ),
    ]);

    assert!(
        !fixture_receiver_violations(fixture.path()).is_empty(),
        "UFCS through an AgentController alias cannot bypass the receiver guard"
    );
}

#[test]
fn receiver_reachability_rejects_aliased_unix_listener_acceptance() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "use std::os::unix::net::UnixListener as Endpoint;\npub fn dispatch(listener: &Endpoint) { let _ = Endpoint::accept(listener); }\n",
        ),
    ]);

    assert!(
        !fixture_receiver_violations(fixture.path()).is_empty(),
        "an aliased UnixListener accept is still a receiver socket consumer"
    );
}

#[test]
fn receiver_reachability_rejects_aliased_ufcs_socket_reads() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "use std::io::Read as SocketRead;\nuse std::os::unix::net::UnixStream as Endpoint;\npub struct InboundJob;\npub fn dispatch(stream: &mut Endpoint, _job: &InboundJob) { let mut bytes = [0_u8; 8]; let _ = SocketRead::read(stream, &mut bytes); }\n",
        ),
    ]);

    assert!(
        !fixture_receiver_violations(fixture.path()).is_empty(),
        "an aliased Read UFCS call cannot hide an inbound Unix socket consumer"
    );
}

#[test]
fn receiver_reachability_rejects_channel_and_memory_queue_consumers() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "receiver.rs",
            "pub fn dispatch(inbox: crate::neutral::Inbox, jobs: crate::neutral::Jobs) { crate::neutral::consume(inbox, jobs); }\n",
        ),
        (
            "neutral.rs",
            "use crate::server::receiver::job::InboundJob;\nuse std::collections::VecDeque as Buffer;\nuse std::sync::mpsc::Receiver as Channel;\npub type Inbox = Channel<InboundJob>;\npub type Jobs = Buffer<InboundJob>;\npub fn consume(inbox: Inbox, mut jobs: Jobs) { let _ = inbox.recv(); let _ = jobs.pop_front(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.len() >= 2,
        "receiver-reachable channel and in-memory job consumers must both be rejected: {violations:?}"
    );
}

#[test]
fn ordinary_interactive_and_exact_cfg_test_calls_are_not_receiver_reachable() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod interactive;\n#[cfg(test)] mod hidden;\n"),
        (
            "interactive.rs",
            "pub fn submit(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n#[cfg(test)] fn hidden(controller: &mut crate::agent::controller::AgentController) { controller.type_text(\"test\"); }\n",
        ),
        (
            "hidden.rs",
            "pub fn test_only(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n",
        ),
    ]);

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "ordinary interactive APIs and exact cfg(test) scopes stay outside receiver ownership"
    );
}

#[test]
fn lifetime_only_job_socket_ownership_is_not_a_consumer() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "use crate::tui::singleton::JobSocket as Endpoint;\npub fn bind(workspace: &crate::WorkspaceContext) -> Endpoint { Endpoint::bind(workspace).unwrap() }\npub fn own(socket: Endpoint) { drop(socket); }\n",
        ),
    ]);

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "binding, owning, and dropping a lifetime-only JobSocket is not job consumption"
    );
}

fn rust_fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
    let fixture = tempfile::tempdir().expect("create receiver architecture fixture");
    let src = fixture.path().join("src");
    std::fs::create_dir_all(&src).expect("create fixture source");
    for (relative, source) in files {
        let path = src.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture module parent");
        }
        std::fs::write(path, source).expect("write receiver architecture fixture");
    }
    fixture
}

fn fixture_receiver_violations(root: &Path) -> Vec<String> {
    analysis::receiver_violations(root)
}

fn has_dead_code_allowance(attribute: &syn::Attribute) -> bool {
    let syn::Meta::List(meta) = &attribute.meta else {
        return false;
    };
    meta.path.is_ident("allow")
        && meta
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|paths| paths.iter().any(|path| path.is_ident("dead_code")))
}

#[derive(Default)]
struct DeadCodeAllowance {
    found: bool,
}

impl<'ast> syn::visit::Visit<'ast> for DeadCodeAllowance {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        self.found |= has_dead_code_allowance(attribute);
    }
}
