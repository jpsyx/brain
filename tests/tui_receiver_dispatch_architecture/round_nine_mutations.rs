use super::{analysis, fixture_receiver_violations, rust_fixture};

#[test]
fn block_alias_resolves_the_brain_controller() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local = crate::agent::controller::AgentController; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn nested_block_alias_chain_resolves_the_brain_controller() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local<T> = Middle<T>; type Middle<U> = U; let controller: &mut Local<crate::agent::controller::AgentController> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn generic_block_alias_substitutes_the_brain_controller() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local<T> = T; let controller: &mut Local<crate::agent::controller::AgentController> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn block_alias_marks_the_canonical_inbound_job() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Job = crate::server::receiver::job::InboundJob; type Inbox = std::sync::mpsc::Receiver<Job>; let inbox: Inbox = unreachable!(); let _ = inbox.recv(); }\n",
    );
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("receiver channel consume")),
        "a block alias to the canonical inbound job must remain guarded: {violations:?}"
    );
}

#[test]
fn block_alias_marks_the_canonical_brain_panel() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Panel = crate::tui::state::brain::BrainPanelState; let panel: &mut Panel = unreachable!(); panel.take_main(); }\n",
    );
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("main-panel controller access")),
        "a block alias to the canonical brain panel must remain guarded: {violations:?}"
    );
}

#[test]
fn block_alias_marks_the_canonical_tui_consumer() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Shell = crate::tui::App; let app: &mut Shell = unreachable!(); Shell::tick_receiver(app); }\n",
    );

    assert_eq!(
        analysis::receiver_tick_call_count(fixture.path()),
        1,
        "a block alias to the canonical TUI App must count as the durable consumer"
    );
}

#[test]
fn block_alias_item_is_visible_before_its_statement() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { let controller: &mut Local = unreachable!(); controller.submit_now(); type Local = crate::agent::controller::AgentController; }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn inner_alias_shadows_then_unwinds_to_the_outer_alias() {
    let fixture = receiver_fixture(
        "pub struct Harmless; impl Harmless { pub fn submit_now(&mut self) {} } pub fn dispatch() { type Local = crate::agent::controller::AgentController; { type Local = Harmless; let safe: &mut Local = unreachable!(); safe.submit_now(); } let guarded: &mut Local = unreachable!(); guarded.submit_now(); }\n",
    );
    let violations = fixture_receiver_violations(fixture.path());

    assert_eq!(
        controller_violation_count(&violations),
        1,
        "the inner harmless alias must shadow only until its block unwinds: {violations:?}"
    );
}

#[test]
fn inner_opaque_type_shadows_an_outer_alias() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local = crate::agent::controller::AgentController; { struct Local; impl Local { fn submit_now(&mut self) {} } let controller: &mut Local = unreachable!(); controller.submit_now(); } }\n",
    );

    assert_no_violations(
        fixture.path(),
        "an inner opaque type declaration must stop outer alias lookup",
    );
}

#[test]
fn alias_target_uses_its_definition_scope() {
    let fixture = agent_fixture(
        "use crate::agent::*; pub fn dispatch() { type Local = AgentController; { struct AgentController; impl AgentController { fn submit_now(&mut self) {} } let controller: &mut Local = unreachable!(); controller.submit_now(); } }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn block_alias_to_a_local_same_named_type_is_harmless() {
    let fixture = agent_fixture(
        "use crate::agent::*; pub fn dispatch() { struct AgentController; impl AgentController { fn submit_now(&mut self) {} } type Local = AgentController; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "a block alias to a local same-named type is not Brain's controller",
    );
}

#[test]
fn alias_generic_shadows_a_same_named_glob_export() {
    let fixture = agent_fixture(
        "use crate::agent::*; pub trait LocalSubmit { fn submit_now(&mut self); } pub fn dispatch<Harmless: LocalSubmit>(controller: &mut Harmless) { type Local<AgentController> = AgentController; let controller: &mut Local<Harmless> = controller; controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "an alias generic must resolve before the same-named module glob export",
    );
}

#[test]
fn alias_generic_does_not_leak_into_a_module_alias_target() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod harmless;\nmod receiver;\n"),
        (
            "harmless.rs",
            "pub struct T; impl T { pub fn submit_now(&mut self) {} } pub type Wrapper = T;\n",
        ),
        (
            "receiver.rs",
            "pub fn dispatch() { type Local<T> = crate::harmless::Wrapper; let controller: &mut Local<crate::agent::controller::AgentController> = unreachable!(); controller.submit_now(); }\n",
        ),
    ]);

    assert_no_violations(
        fixture.path(),
        "a lexical alias binding must not escape into a module alias definition",
    );
}

#[test]
fn cyclic_block_aliases_terminate_without_a_false_positive() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type First = Second; type Second = First; let controller: &mut First = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "a cycle of opaque block aliases must terminate without inventing a Brain type",
    );
}

#[test]
fn ambiguous_alias_target_fails_closed() {
    let fixture = agent_and_decoy_fixture(
        "pub fn dispatch() { use crate::agent::*; use crate::decoy::*; type Local = AgentController; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_unresolved_alias_violation(fixture.path());
}

#[test]
fn unknown_alias_target_fails_closed() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod names;\nmod receiver;\n"),
        ("names.rs", "pub struct DifferentController;\n"),
        (
            "receiver.rs",
            "pub fn dispatch() { use crate::names::*; type Local = AgentController; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
        ),
    ]);

    assert_has_unresolved_alias_violation(fixture.path());
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
        controller_violation_count(&violations) > 0,
        "the block alias to Brain's controller must remain guarded: {violations:?}"
    );
}

fn assert_has_unresolved_alias_violation(root: &std::path::Path) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("unresolved glob-owned type")),
        "an unresolved block-alias target must fail closed: {violations:?}"
    );
}

fn assert_no_violations(root: &std::path::Path, message: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(violations.is_empty(), "{message}: {violations:?}");
}

fn controller_violation_count(violations: &[String]) -> usize {
    violations
        .iter()
        .filter(|violation| violation.contains("AgentController submit_now"))
        .count()
}
