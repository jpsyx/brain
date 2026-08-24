use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn neutral_named_undeclared_source_is_an_audited_orphan_root() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod ordinary;\n"),
        ("ordinary.rs", "pub fn live() {}\n"),
        (
            "transport.rs",
            "use std::os::unix::net::UnixListener as Endpoint;\npub fn consume(listener: &Endpoint) { let _ = listener.accept(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("UnixListener accept")),
        "an undeclared production orphan is audited independently of its filename: {violations:?}"
    );
}

#[test]
fn plain_receiver_unix_stream_read_needs_no_job_parameter() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "use std::io::Read;\nuse std::os::unix::net::UnixStream;\npub fn consume(stream: &mut UnixStream) { let mut bytes = [0_u8; 8]; let _ = stream.read(&mut bytes); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("Unix socket read")),
        "a receiver-owned UnixStream read is consumption before any job is decoded: {violations:?}"
    );
}

#[test]
fn controller_type_aliases_resolve_across_modules() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod aliases;\nmod receiver;\n"),
        (
            "aliases.rs",
            "pub type Frontend = crate::agent::controller::AgentController;\n",
        ),
        (
            "receiver.rs",
            "use crate::aliases::Frontend;\npub fn dispatch(controller: &mut Frontend) { controller.submit_now(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "a cross-module controller alias preserves its exact type ownership: {violations:?}"
    );
}

#[test]
fn qualified_self_ufcs_resolves_the_controller_owner() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "use crate::agent::controller::AgentController as Frontend;\npub trait Input { fn type_text(&mut self, text: &str); }\npub fn dispatch(controller: &mut Frontend) { <Frontend as Input>::type_text(controller, \"remote\"); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController type_text")),
        "qualified-self UFCS cannot hide controller input ownership: {violations:?}"
    );
}

#[test]
fn controller_fields_preserve_type_through_member_access() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "pub struct Run { pub controller: crate::agent::controller::AgentController }\npub fn dispatch(run: &mut Run) { run.controller.submit_now(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "field access preserves the controller's exact type: {violations:?}"
    );
}

#[test]
fn returned_controller_method_chains_preserve_type() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "neutral.rs",
            "pub fn controller(value: &mut crate::agent::controller::AgentController) -> &mut crate::agent::controller::AgentController { value }\n",
        ),
        (
            "receiver.rs",
            "pub fn dispatch(value: &mut crate::agent::controller::AgentController) { crate::neutral::controller(value).type_text(\"remote\"); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController type_text")),
        "a returned controller remains typed across a method chain: {violations:?}"
    );
}

#[test]
fn unrelated_same_basename_method_is_not_a_call_edge() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "neutral.rs",
            "pub fn drive(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n",
        ),
        (
            "receiver.rs",
            "pub trait SafeAction { fn drive(&mut self); }\npub fn dispatch<T: SafeAction>(safe: &mut T) { safe.drive(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.is_empty(),
        "an unrelated method cannot reach a free function by basename alone: {violations:?}"
    );
}
