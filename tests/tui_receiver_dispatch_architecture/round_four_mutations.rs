use super::{analysis, fixture_receiver_violations, rust_fixture};

#[test]
fn inferred_controller_local_retains_its_returned_type() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "neutral.rs",
            "pub fn controller(value: &mut crate::agent::controller::AgentController) -> &mut crate::agent::controller::AgentController { value }\n",
        ),
        (
            "receiver.rs",
            "pub fn dispatch(value: &mut crate::agent::controller::AgentController) { let controller = crate::neutral::controller(value); controller.submit_now(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "an inferred local must retain the exact returned controller type: {violations:?}"
    );
}

#[test]
fn inferred_non_controller_local_does_not_inherit_a_same_named_operation() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "neutral.rs",
            "pub struct Worker;\nimpl Worker { pub fn submit_now(&mut self) {} }\npub fn worker() -> Worker { Worker }\n",
        ),
        (
            "receiver.rs",
            "pub fn dispatch() { let mut worker = crate::neutral::worker(); worker.submit_now(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.is_empty(),
        "inferred local typing must not classify by method name alone: {violations:?}"
    );
}

#[test]
fn ordinary_method_dispatch_reaches_the_only_in_scope_trait_impl() {
    let fixture = trait_dispatch_fixture("Unsafe");
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "ordinary method syntax must reach the exact in-scope trait implementation: {violations:?}"
    );
}

#[test]
fn ordinary_safe_trait_dispatch_does_not_inherit_the_unsafe_impl_edge() {
    let fixture = trait_dispatch_fixture("Safe");
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations.is_empty(),
        "ordinary method syntax must not cross into an out-of-scope same-method trait: {violations:?}"
    );
}

#[test]
fn associated_receiver_tick_call_counts_as_the_durable_consumer() {
    let fixture = rust_fixture(&[
        (
            "lib.rs",
            "mod receiver;\nmod tui { pub struct App; impl App { pub fn tick_receiver(&mut self) {} } }\n",
        ),
        (
            "receiver.rs",
            "pub fn dispatch(app: &mut crate::tui::App) { crate::tui::App::tick_receiver(app); }\n",
        ),
    ]);

    assert_eq!(
        analysis::receiver_tick_call_count(fixture.path()),
        1,
        "associated App receiver dispatch is the same durable-consumer call"
    );
}

#[test]
fn unrelated_same_named_method_is_not_a_receiver_tick() {
    let fixture = rust_fixture(&[
        (
            "lib.rs",
            "pub struct Worker;\nimpl Worker { pub fn tick_receiver(&mut self) {} }\nmod receiver;\n",
        ),
        (
            "receiver.rs",
            "pub fn dispatch(worker: &mut crate::Worker) { worker.tick_receiver(); }\n",
        ),
    ]);

    assert_eq!(
        analysis::receiver_tick_call_count(fixture.path()),
        0,
        "receiver-consumer counting must use the App owner, not a method-name token"
    );
}

fn trait_dispatch_fixture(selected_trait: &str) -> tempfile::TempDir {
    let receiver = format!(
        "use crate::neutral::{{{selected_trait}, Worker}};\n\
         pub fn dispatch(worker: &mut Worker<'_>) {{ worker.drive(); }}\n"
    );
    rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "neutral.rs",
            "pub trait Safe { fn drive(&mut self); }\n\
             pub trait Unsafe { fn drive(&mut self); }\n\
             pub struct Worker<'a> { controller: &'a mut crate::agent::controller::AgentController }\n\
             impl Unsafe for Worker<'_> { fn drive(&mut self) { inject(self.controller); } }\n\
             impl Safe for Worker<'_> { fn drive(&mut self) {} }\n\
             fn inject(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n",
        ),
        ("receiver.rs", &receiver),
    ])
}
