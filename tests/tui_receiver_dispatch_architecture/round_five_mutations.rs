use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn ordinary_dispatch_reaches_a_trait_impl_declared_for_a_generic_alias() {
    let fixture = ordinary_alias_fixture("Unsafe");
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "an ordinary call must reach the implementation registered through its aliased self type: {violations:?}"
    );
}

#[test]
fn ordinary_safe_alias_dispatch_does_not_inherit_the_unsafe_edge() {
    let fixture = ordinary_alias_fixture("Safe");
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations.is_empty(),
        "canonicalizing the impl owner must preserve exact trait selection: {violations:?}"
    );
}

#[test]
fn qself_alias_dispatch_propagates_the_trait_return_type() {
    let fixture = qself_return_fixture("UnsafeView");
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "a qself call through an aliased impl owner must retain its controller return fact: {violations:?}"
    );
}

#[test]
fn qself_safe_alias_return_does_not_become_a_controller() {
    let fixture = qself_return_fixture("SafeView");
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations.is_empty(),
        "a safe qself return with the same method name must retain its harmless type: {violations:?}"
    );
}

#[test]
fn direct_worker_inherent_dispatch_keeps_its_existing_edge() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\nmod worker;\n"),
        (
            "worker.rs",
            "pub struct Worker<'a> { pub controller: &'a mut crate::AgentController }\n\
             impl Worker<'_> { pub fn drive(&mut self) { self.controller.submit_now(); } }\n",
        ),
        (
            "receiver.rs",
            "use crate::worker::Worker;\npub fn dispatch(worker: &mut Worker<'_>) { worker.drive(); }\n",
        ),
    ]);
    let violations = fixture_receiver_violations(fixture.path());

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "direct inherent Worker dispatch must remain exact: {violations:?}"
    );
}

fn ordinary_alias_fixture(selected_trait: &str) -> tempfile::TempDir {
    let receiver = format!(
        "use crate::a_impl::{selected_trait};\n\
         use crate::z_model::Runner;\n\
         pub fn dispatch(runner: &mut Runner<'_>) {{ runner.drive(); }}\n"
    );
    rust_fixture(&[
        ("lib.rs", "mod a_impl;\nmod receiver;\nmod z_model;\n"),
        (
            "a_impl.rs",
            "use crate::z_model::Runner;\n\
             pub trait Safe { fn drive(&mut self); }\n\
             pub trait Unsafe { fn drive(&mut self); }\n\
             impl Unsafe for Runner<'_> { fn drive(&mut self) { inject(self.controller); } }\n\
             impl Safe for Runner<'_> { fn drive(&mut self) {} }\n\
             fn inject(controller: &mut crate::AgentController) { controller.submit_now(); }\n",
        ),
        ("receiver.rs", &receiver),
        (
            "z_model.rs",
            "pub struct Worker<'a> { pub controller: &'a mut crate::AgentController }\n\
             pub type Runner<'a> = Worker<'a>;\n",
        ),
    ])
}

fn qself_return_fixture(selected_trait: &str) -> tempfile::TempDir {
    let receiver = format!(
        "use crate::a_impl::{selected_trait};\n\
         use crate::z_model::Runner;\n\
         pub fn dispatch(runner: &mut Runner<'_>) {{\n\
             <Runner<'_> as {selected_trait}>::controller(runner).submit_now();\n\
         }}\n"
    );
    rust_fixture(&[
        ("lib.rs", "mod a_impl;\nmod receiver;\nmod z_model;\n"),
        (
            "a_impl.rs",
            "use crate::z_model::{Harmless, Runner};\n\
             pub trait SafeView { fn controller(&mut self) -> &mut Harmless; }\n\
             pub trait UnsafeView { fn controller(&mut self) -> &mut crate::AgentController; }\n\
             impl UnsafeView for Runner<'_> {\n\
                 fn controller(&mut self) -> &mut crate::AgentController { self.controller }\n\
             }\n\
             impl SafeView for Runner<'_> {\n\
                 fn controller(&mut self) -> &mut Harmless { self.harmless }\n\
             }\n",
        ),
        ("receiver.rs", &receiver),
        (
            "z_model.rs",
            "pub struct Harmless;\n\
             impl Harmless { pub fn submit_now(&mut self) {} }\n\
             pub struct Worker<'a> {\n\
                 pub controller: &'a mut crate::AgentController,\n\
                 pub harmless: &'a mut Harmless,\n\
             }\n\
             pub type Runner<'a> = Worker<'a>;\n",
        ),
    ])
}
