use super::{analysis, fixture_receiver_violations, rust_fixture};

#[test]
fn omitted_default_resolves_the_brain_controller() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local<T = crate::agent::controller::AgentController> = T; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn partial_arguments_resolve_a_later_brain_controller_default() {
    let fixture = receiver_fixture(
        "pub struct Harmless; pub fn dispatch() { type Local<T, U = crate::agent::controller::AgentController> = U; let controller: &mut Local<Harmless> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn chained_defaults_resolve_through_an_earlier_parameter() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local<T = crate::agent::controller::AgentController, U = T> = U; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn lifetime_and_const_arguments_do_not_shift_a_later_type_default() {
    let fixture = receiver_fixture(
        "pub struct Harmless; pub fn dispatch() { type Local<'a, T, const N: usize, U = crate::agent::controller::AgentController> = U; let controller: &mut Local<'static, Harmless, 4> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn omitted_default_resolves_the_canonical_inbound_job() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Job<T = crate::server::receiver::job::InboundJob> = T; type Inbox<T = Job> = std::sync::mpsc::Receiver<T>; let inbox: Inbox = unreachable!(); let _ = inbox.recv(); }\n",
    );
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("receiver channel consume")),
        "a default to the canonical inbound job must remain guarded: {violations:?}"
    );
}

#[test]
fn omitted_default_resolves_the_canonical_brain_panel() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Panel<T = crate::tui::state::brain::BrainPanelState> = T; let panel: &mut Panel = unreachable!(); panel.take_main(); }\n",
    );
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("main-panel controller access")),
        "a default to the canonical brain panel must remain guarded: {violations:?}"
    );
}

#[test]
fn omitted_default_resolves_the_canonical_tui_consumer() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Shell<T = crate::tui::App> = T; let app: &mut Shell = unreachable!(); Shell::tick_receiver(app); }\n",
    );

    assert_eq!(
        analysis::receiver_tick_call_count(fixture.path()),
        1,
        "a default to the canonical TUI App must count as the durable consumer"
    );
}

#[test]
fn explicit_type_argument_overrides_a_brain_controller_default() {
    let fixture = receiver_fixture(
        "pub struct Harmless; impl Harmless { pub fn submit_now(&mut self) {} } pub fn dispatch() { type Local<T = crate::agent::controller::AgentController> = T; let controller: &mut Local<Harmless> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "an explicit harmless type must override the guarded default",
    );
}

#[test]
fn explicit_later_type_argument_stays_aligned_after_lifetime_and_const_arguments() {
    let fixture = receiver_fixture(
        "pub struct Harmless; impl Harmless { pub fn submit_now(&mut self) {} } pub fn dispatch() { type Local<'a, T, const N: usize, U = crate::agent::controller::AgentController> = U; let controller: &mut Local<'static, Harmless, 4, Harmless> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "lifetime and const arguments must not shift an explicit harmless type argument",
    );
}

#[test]
fn omitted_parameter_without_a_default_remains_conservatively_opaque() {
    let fixture = receiver_fixture(
        "pub trait Submit { fn submit_now(&mut self); } pub fn dispatch() { type Local<T> = T; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "an omitted parameter without a default must not invent a Brain type",
    );
}

#[test]
fn unknown_default_target_fails_closed() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod names;\nmod receiver;\n"),
        ("names.rs", "pub struct DifferentController;\n"),
        (
            "receiver.rs",
            "pub fn dispatch() { use crate::names::*; type Local<T = AgentController> = T; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
        ),
    ]);

    assert_has_unresolved_alias_violation(fixture.path());
}

#[test]
fn ambiguous_default_target_fails_closed() {
    let fixture = agent_and_decoy_fixture(
        "pub fn dispatch() { use crate::agent::*; use crate::decoy::*; type Local<T = AgentController> = T; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_unresolved_alias_violation(fixture.path());
}

#[test]
fn nested_default_alias_chain_resolves_the_brain_controller() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Inner<T = crate::agent::controller::AgentController> = T; type Outer<T = Inner> = T; let controller: &mut Outer = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn alias_generic_shadows_a_same_named_default_import() {
    let fixture = agent_fixture(
        "use crate::agent::*; pub struct Harmless; impl Harmless { pub fn submit_now(&mut self) {} } pub fn dispatch() { type Local<AgentController = Harmless> = AgentController; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "an alias generic default must resolve before a same-named glob export",
    );
}

#[test]
fn cyclic_alias_defaults_terminate_without_a_false_positive() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type First<T = Second> = T; type Second<U = First> = U; let controller: &mut First = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "cyclic alias defaults must terminate without inventing a Brain type",
    );
}

fn receiver_fixture(receiver: &str) -> tempfile::TempDir {
    rust_fixture(&[("lib.rs", "mod receiver;\n"), ("receiver.rs", receiver)])
}

fn agent_fixture(receiver: &str) -> tempfile::TempDir {
    rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\npub use controller::*;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController; impl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("receiver.rs", receiver),
    ])
}

fn agent_and_decoy_fixture(receiver: &str) -> tempfile::TempDir {
    rust_fixture(&[
        ("lib.rs", "mod agent;\nmod decoy;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\npub use controller::*;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController; impl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "decoy.rs",
            "pub struct AgentController; impl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("receiver.rs", receiver),
    ])
}

fn assert_has_controller_violation(root: &std::path::Path) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "the default to Brain's controller must remain guarded: {violations:?}"
    );
}

fn assert_has_unresolved_alias_violation(root: &std::path::Path) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("unresolved glob-owned type")),
        "an unresolved block-alias default must fail closed: {violations:?}"
    );
}

fn assert_no_violations(root: &std::path::Path, message: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(violations.is_empty(), "{message}: {violations:?}");
}
