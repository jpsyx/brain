use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn function_block_glob_resolves_the_brain_controller() {
    let fixture = agent_fixture(
        "pub fn dispatch() { use crate::agent::*; let controller: &mut AgentController = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path(), "submit_now");
}

#[test]
fn nested_block_glob_resolves_the_brain_controller() {
    let fixture = agent_fixture(
        "pub fn dispatch() { if true { use crate::agent::*; let controller: &mut AgentController = unreachable!(); controller.type_text(\"remote\"); } }\n",
    );

    assert_has_controller_violation(fixture.path(), "type_text");
}

#[test]
fn inner_named_import_shadows_an_outer_module_glob() {
    let fixture = agent_and_decoy_fixture(
        "use crate::agent::*;\npub fn dispatch() { use crate::decoy::AgentController; let controller: &mut AgentController = unreachable!(); controller.submit_now(); }\n",
    );

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a block-local named import must shadow an outer module glob"
    );
}

#[test]
fn inner_glob_shadows_an_outer_block_glob() {
    let fixture = agent_and_decoy_fixture(
        "pub fn dispatch() { use crate::agent::*; { use crate::decoy::*; let controller: &mut AgentController = unreachable!(); controller.submit_now(); } }\n",
    );

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "the nearest lexical glob scope must decide the same-named type"
    );
}

#[test]
fn nested_block_glob_does_not_leak_into_its_sibling() {
    let fixture = agent_fixture(
        "pub fn dispatch() { { use crate::agent::*; } { struct AgentController; impl AgentController { fn submit_now(&mut self) {} } let controller: &mut AgentController = unreachable!(); controller.submit_now(); } }\n",
    );

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a nested lexical glob must not escape into its sibling block"
    );
}

#[test]
fn same_named_function_generic_shadows_a_module_glob() {
    let fixture = agent_fixture(
        "use crate::agent::*;\npub trait LocalSubmit { fn submit_now(&mut self); }\npub fn dispatch<AgentController: LocalSubmit>(controller: &mut AgentController) { controller.submit_now(); }\n",
    );

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a function generic named AgentController is not the imported Brain type"
    );
}

#[test]
fn method_generic_shadows_an_unknown_module_glob_symbol() {
    let fixture = agent_fixture(
        "use crate::agent::*;\npub trait LocalSubmit { fn submit_now(&mut self); }\npub struct Receiver;\nimpl Receiver { pub fn dispatch<T: LocalSubmit>(&self, controller: &mut T) { controller.submit_now(); } }\n",
    );

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a method generic T must not fail closed as an unknown glob-owned type"
    );
}

#[test]
fn impl_generic_shadows_an_unknown_module_glob_symbol() {
    let fixture = agent_fixture(
        "use crate::agent::*;\npub trait LocalSubmit { fn submit_now(&mut self); }\npub struct Receiver<T>(T);\nimpl<T: LocalSubmit> Receiver<T> { pub fn dispatch(&mut self) { self.0.submit_now(); } }\n",
    );

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "an impl generic T must remain local when its field is used"
    );
}

#[test]
fn block_local_same_named_type_shadows_a_module_glob() {
    let fixture = agent_fixture(
        "use crate::agent::*;\npub fn dispatch() { struct AgentController; impl AgentController { fn submit_now(&mut self) {} } let controller: &mut AgentController = unreachable!(); controller.submit_now(); }\n",
    );

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a block-local same-named type must resolve before the module glob"
    );
}

#[test]
fn ambiguous_block_globs_fail_closed() {
    let fixture = agent_and_decoy_fixture(
        "pub fn dispatch() { use crate::agent::*; use crate::decoy::*; let controller: &mut AgentController = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_unresolved_glob_violation(fixture.path());
}

#[test]
fn unknown_block_glob_type_fails_closed() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod names;\nmod receiver;\n"),
        ("names.rs", "pub struct DifferentController;\n"),
        (
            "receiver.rs",
            "pub fn dispatch() { use crate::names::*; let controller: &mut AgentController = unreachable!(); controller.submit_now(); }\n",
        ),
    ]);

    assert_has_unresolved_glob_violation(fixture.path());
}

fn agent_fixture(receiver: &str) -> tempfile::TempDir {
    rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\npub use controller::*;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} pub fn type_text(&mut self, _text: &str) {} }\n",
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
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "decoy.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("receiver.rs", receiver),
    ])
}

fn assert_has_controller_violation(root: &std::path::Path, method: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&format!("AgentController {method}"))),
        "the lexical Brain controller call {method} must remain guarded: {violations:?}"
    );
}

fn assert_has_unresolved_glob_violation(root: &std::path::Path) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("unresolved glob-owned type")),
        "an unresolved lexical glob type must fail closed: {violations:?}"
    );
}
