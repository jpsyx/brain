use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn module_glob_trait_dispatch_reaches_the_forbidden_impl() {
    let fixture = glob_trait_fixture("use crate::behavior::*;");
    assert_forbidden_trait_edge(fixture.path(), "module glob");
}

#[test]
fn block_glob_trait_dispatch_reaches_the_forbidden_impl() {
    let fixture = glob_trait_fixture("");
    let receiver = fixture.path().join("src/receiver.rs");
    std::fs::write(
        receiver,
        "use crate::worker::Worker;\npub fn dispatch(worker: &mut Worker<'_>) { use crate::behavior::*; worker.drive(); }\n",
    )
    .expect("write block-glob receiver fixture");
    assert_forbidden_trait_edge(fixture.path(), "block glob");
}

#[test]
fn ambiguous_glob_traits_do_not_guess_a_forbidden_impl() {
    let fixture = rust_fixture(&[
        (
            "lib.rs",
            "mod agent;\nmod behavior_a;\nmod behavior_b;\nmod worker;\nmod receiver;\n",
        ),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "behavior_a.rs",
            "pub trait Drive { fn drive(&mut self); }\n",
        ),
        (
            "behavior_b.rs",
            "pub trait Drive { fn drive(&mut self); }\n",
        ),
        (
            "worker.rs",
            "pub struct Worker<'a> { controller: &'a mut crate::agent::controller::AgentController }\nimpl crate::behavior_a::Drive for Worker<'_> { fn drive(&mut self) { self.controller.submit_now(); } }\nimpl crate::behavior_b::Drive for Worker<'_> { fn drive(&mut self) {} }\n",
        ),
        (
            "receiver.rs",
            "use crate::behavior_a::*;\nuse crate::behavior_b::*;\nuse crate::worker::Worker;\npub fn dispatch(worker: &mut Worker<'_>) { worker.drive(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.is_empty(),
        "ambiguous trait globs must not guess one implementation edge: {violations:?}"
    );
}

#[test]
fn local_trait_declaration_shadows_a_module_glob() {
    let fixture = rust_fixture(&[
        (
            "lib.rs",
            "mod agent;\nmod behavior;\nmod worker;\nmod receiver;\n",
        ),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("behavior.rs", "pub trait Drive { fn drive(&mut self); }\n"),
        (
            "worker.rs",
            "pub struct Worker<'a> { controller: &'a mut crate::agent::controller::AgentController }\nimpl crate::behavior::Drive for Worker<'_> { fn drive(&mut self) { self.controller.submit_now(); } }\n",
        ),
        (
            "receiver.rs",
            "use crate::behavior::*;\nuse crate::worker::Worker;\ntrait Drive { fn drive(&mut self); }\nimpl Drive for Worker<'_> { fn drive(&mut self) {} }\npub fn dispatch(worker: &mut Worker<'_>) { worker.drive(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.is_empty(),
        "a local trait declaration must shadow a module glob: {violations:?}"
    );
}

#[test]
fn declared_neutral_direct_inbound_consumer_is_globally_rejected() {
    let fixture = declared_neutral_consumer_fixture(
        "pub fn drain(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>) { let _ = inbox.recv(); }\n",
    );
    assert_inbound_consumer_violation(fixture.path(), "direct declared-neutral consumer");
}

#[test]
fn declared_neutral_indirect_inbound_consumer_is_globally_rejected() {
    let fixture = declared_neutral_consumer_fixture(
        "pub fn drain(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>) { helper(inbox); }\nfn helper(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>) { let _ = inbox.try_recv(); }\n",
    );
    assert_inbound_consumer_violation(fixture.path(), "indirect declared-neutral consumer");
}

#[test]
fn unrelated_declared_channel_and_socket_consumers_stay_outside_receiver_ownership() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\n"),
        (
            "neutral.rs",
            "use std::io::Read as _;\npub fn ordinary(inbox: std::sync::mpsc::Receiver<String>, stream: &mut std::os::unix::net::UnixStream) { let _ = inbox.recv(); let mut bytes = [0_u8; 8]; let _ = stream.read(&mut bytes); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.is_empty(),
        "unrelated ordinary channel and socket consumers must remain allowed: {violations:?}"
    );
}

#[test]
fn declared_neutral_inbound_value_without_consumption_is_allowed() {
    let fixture = declared_neutral_consumer_fixture(
        "pub fn own(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>) { drop(inbox); }\n",
    );

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.is_empty(),
        "owning a typed inbound receiver without consuming it must stay allowed: {violations:?}"
    );
}

fn glob_trait_fixture(receiver_import: &str) -> tempfile::TempDir {
    let receiver = format!(
        "{receiver_import}\nuse crate::worker::Worker;\npub fn dispatch(worker: &mut Worker<'_>) {{ worker.drive(); }}\n"
    );
    rust_fixture(&[
        (
            "lib.rs",
            "mod agent;\nmod behavior;\nmod worker;\nmod receiver;\n",
        ),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("behavior.rs", "pub trait Drive { fn drive(&mut self); }\n"),
        (
            "worker.rs",
            "pub struct Worker<'a> { controller: &'a mut crate::agent::controller::AgentController }\nimpl crate::behavior::Drive for Worker<'_> { fn drive(&mut self) { inject(self.controller); } }\nfn inject(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n",
        ),
        ("receiver.rs", &receiver),
    ])
}

fn declared_neutral_consumer_fixture(neutral: &str) -> tempfile::TempDir {
    rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod server;\n"),
        ("neutral.rs", neutral),
        (
            "server.rs",
            "pub mod receiver { pub mod job { pub struct InboundJob; } }\n",
        ),
    ])
}

fn assert_forbidden_trait_edge(root: &std::path::Path, scope: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "a {scope} must make the exact trait implementation reachable: {violations:?}"
    );
}

fn assert_inbound_consumer_violation(root: &std::path::Path, case: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("receiver channel consume")),
        "the global typed-consumer audit must reject a {case}: {violations:?}"
    );
}
