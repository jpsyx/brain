use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn nested_public_and_private_globs_resolve_the_brain_controller() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\npub use controller::*;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "receiver.rs",
            "use crate::agent::*;\npub fn dispatch(controller: &mut AgentController) { controller.submit_now(); }\n",
        ),
    ]);

    assert!(
        fixture_receiver_violations(fixture.path())
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "a public controller glob followed by a receiver-local glob must retain the canonical Brain type"
    );
}

#[test]
fn direct_module_glob_resolves_the_brain_controller() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn type_text(&mut self, _text: &str) {} }\n",
        ),
        (
            "receiver.rs",
            "use crate::agent::controller::*;\npub fn dispatch(controller: &mut AgentController) { controller.type_text(\"remote\"); }\n",
        ),
    ]);

    assert!(
        fixture_receiver_violations(fixture.path())
            .iter()
            .any(|violation| violation.contains("AgentController type_text")),
        "a direct glob import from the canonical controller module must stay guarded"
    );
}

#[test]
fn cyclic_globs_do_not_override_a_receiver_local_same_named_type() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod a;\nmod b;\nmod receiver;\n"),
        ("a.rs", "pub use crate::b::*;\n"),
        ("b.rs", "pub use crate::a::*;\n"),
        (
            "receiver.rs",
            "use crate::a::*;\npub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\npub fn dispatch(controller: &mut AgentController) { controller.submit_now(); }\n",
        ),
    ]);

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a finite glob cycle must not displace a receiver-local same-named type"
    );
}

#[test]
fn ambiguous_glob_exports_fail_closed_without_guessing_one_identity() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod agent;\nmod decoy;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\npub use controller::*;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "decoy.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "receiver.rs",
            "use crate::agent::*;\nuse crate::decoy::*;\npub fn dispatch(controller: &mut AgentController) { controller.submit_now(); }\n",
        ),
    ]);

    assert!(
        fixture_receiver_violations(fixture.path())
            .iter()
            .any(|violation| violation.contains("unresolved glob-owned type")),
        "ambiguous glob exports must fail closed instead of guessing one identity"
    );
}

#[test]
fn unknown_glob_owned_type_fails_closed() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub struct DifferentController;\n"),
        (
            "receiver.rs",
            "use crate::agent::*;\npub fn dispatch(controller: &mut AgentController) { controller.submit_now(); }\n",
        ),
    ]);

    assert!(
        fixture_receiver_violations(fixture.path())
            .iter()
            .any(|violation| violation.contains("unresolved glob-owned type")),
        "an unknown type supplied only through a glob must fail closed"
    );
}

#[test]
fn a_dangerous_server_client_method_remains_reachable() {
    let fixture = server_client_fixture(
        "pub fn dispatch(client: &crate::server::control::ServerClient, inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>, stream: &mut std::os::unix::net::UnixStream) { client.consume(inbox, stream); }\n",
    );
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("receiver channel consume")),
        "a non-refresh ServerClient method cannot hide inbound channel consumption: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("Unix socket read")),
        "a non-refresh ServerClient method cannot hide a delegated inbound socket read: {violations:?}"
    );
}

#[test]
fn exact_typed_server_refresh_remains_a_control_capability() {
    let fixture = server_client_fixture(
        "pub fn dispatch(client: &crate::server::control::ServerClient, generation: crate::server::lifecycle::ServerGeneration, workspace_id: crate::workspace::WorkspaceId) { client.refresh_enabled_generation(generation, workspace_id); }\n",
    );

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "the exact typed receiver-intent refresh remains an outbound control capability"
    );
}

#[test]
fn server_refresh_name_with_inbound_types_is_not_a_control_capability() {
    let fixture = server_client_fixture_with_impl(
        "pub fn dispatch(client: &crate::server::control::ServerClient, inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>, stream: &mut std::os::unix::net::UnixStream) { client.refresh_enabled_generation(inbox, stream); }\n",
        "pub fn refresh_enabled_generation(&self, inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>, stream: &mut std::os::unix::net::UnixStream) { crate::neutral::consume(inbox, stream); }",
    );
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("receiver channel consume")),
        "the safe method name cannot exempt an inbound typed operation: {violations:?}"
    );
}

fn server_client_fixture(receiver: &str) -> tempfile::TempDir {
    server_client_fixture_with_impl(
        receiver,
        "pub fn consume(&self, inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>, stream: &mut std::os::unix::net::UnixStream) { crate::neutral::consume(inbox, stream); }\n    pub fn refresh_enabled_generation(&self, _generation: crate::server::lifecycle::ServerGeneration, _workspace_id: crate::workspace::WorkspaceId) { let mut stream = crate::workspace::control_stream(); crate::neutral::read_response(&mut stream); }",
    )
}

fn server_client_fixture_with_impl(receiver: &str, methods: &str) -> tempfile::TempDir {
    let client = format!("pub struct ServerClient;\nimpl ServerClient {{\n    {methods}\n}}\n");
    rust_fixture(&[
        (
            "lib.rs",
            "mod neutral;\nmod receiver;\nmod server;\nmod workspace;\n",
        ),
        (
            "neutral.rs",
            "use std::io::Read as _;\npub fn consume(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>, stream: &mut std::os::unix::net::UnixStream) { let _ = inbox.recv(); let mut bytes = [0_u8; 8]; let _ = stream.read(&mut bytes); }\npub fn read_response(stream: &mut std::os::unix::net::UnixStream) { let mut bytes = [0_u8; 8]; let _ = stream.read(&mut bytes); }\n",
        ),
        ("receiver.rs", receiver),
        (
            "server.rs",
            "pub mod control;\npub mod lifecycle;\npub mod receiver { pub mod job { pub struct InboundJob; } }\n",
        ),
        (
            "server/lifecycle.rs",
            "mod state;\npub use state::ServerGeneration;\n",
        ),
        (
            "server/lifecycle/state.rs",
            "pub struct ServerGeneration;\n",
        ),
        (
            "server/control.rs",
            "mod client;\npub use client::ServerClient;\n",
        ),
        ("server/control/client.rs", &client),
        (
            "workspace.rs",
            "mod id;\npub use id::WorkspaceId;\npub fn control_stream() -> std::os::unix::net::UnixStream { unreachable!() }\n",
        ),
        ("workspace/id.rs", "pub struct WorkspaceId;\n"),
    ])
}
