use super::super::rust_fixture;
use super::support::{
    assert_channel_consumer, assert_controller_violation, assert_no_violations,
    assert_queue_consumer, receiver_fixture,
};

#[test]
fn named_struct_default_field_uses_aligned_use_site_generics() {
    let fixture = receiver_fixture(
        "pub const CAP: usize = 4;\npub struct Holder<'a, const N: usize, T = crate::agent::controller::AgentController> { pub value: T, pub marker: &'a [u8; N] }\npub fn dispatch(Holder { value, .. }: Holder<'static, CAP>) { value.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "named generic default field");
}

#[test]
fn tuple_struct_default_field_uses_aligned_use_site_generics() {
    let fixture = receiver_fixture(
        "pub const CAP: usize = 4;\npub struct Holder<'a, const N: usize, T = crate::agent::controller::AgentController>(pub &'a [u8; N], pub T);\npub fn dispatch(Holder(_, value): Holder<'static, CAP>) { value.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "tuple-struct generic default field");
}

#[test]
fn direct_generic_field_access_uses_the_explicit_controller_argument() {
    let fixture = receiver_fixture(
        "pub struct Holder<T> { pub value: T }\npub fn dispatch(holder: Holder<crate::agent::controller::AgentController>) { holder.value.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "direct generic field access");
}

#[test]
fn generic_collection_fields_preserve_each_inbound_consumer_role() {
    let fixture = receiver_fixture(
        "pub struct Inputs<Q, C> { pub queue: Q, pub channel: C }\npub fn consume(Inputs { mut queue, channel }: Inputs<std::collections::VecDeque<crate::server::receiver::job::InboundJob>, std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>>) { let _ = queue.pop_front(); let _ = channel.recv(); }\n",
    );
    assert_queue_consumer(fixture.path(), "generic VecDeque field");
    assert_channel_consumer(fixture.path(), "generic Receiver field");
}

#[test]
fn explicit_harmless_struct_argument_overrides_the_controller_default() {
    let fixture = receiver_fixture(
        "pub const CAP: usize = 4;\npub struct Holder<'a, const N: usize, T = crate::agent::controller::AgentController> { pub value: T, pub marker: &'a [u8; N] }\npub fn dispatch(Holder { value, .. }: Holder<'static, CAP, Harmless>) { value.submit_now(); }\n",
    );
    assert_no_violations(fixture.path(), "explicit harmless struct argument");
}

#[test]
fn chained_struct_default_uses_an_earlier_type_parameter() {
    let fixture = receiver_fixture(
        "pub struct Holder<T = crate::agent::controller::AgentController, U = T> { pub value: U, pub marker: std::marker::PhantomData<T> }\npub fn dispatch(Holder { value, .. }: Holder) { value.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "chained struct field default");
}

#[test]
fn explicit_struct_argument_resolves_in_the_use_site_module() {
    let fixture = rust_fixture(&[
        (
            "lib.rs",
            "mod agent;\nmod neutral;\nmod receiver;\nmod server;\n",
        ),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("neutral.rs", "pub struct Holder<T> { pub value: T }\n"),
        (
            "receiver.rs",
            "use crate::agent::controller::AgentController as LocalController;\nuse crate::neutral::Holder;\npub fn dispatch(holder: Holder<LocalController>) { holder.value.submit_now(); }\n",
        ),
        (
            "server.rs",
            "pub mod receiver { pub mod job { pub struct InboundJob; } }\n",
        ),
    ]);
    assert_controller_violation(fixture.path(), "cross-module generic use-site argument");
}

#[test]
fn struct_default_resolves_in_the_definition_module() {
    let fixture = rust_fixture(&[
        (
            "lib.rs",
            "mod agent;\nmod neutral;\nmod receiver;\nmod server;\n",
        ),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "neutral.rs",
            "use crate::agent::controller::AgentController as DefaultController;\npub struct Holder<T = DefaultController> { pub value: T }\n",
        ),
        (
            "receiver.rs",
            "use crate::neutral::Holder;\npub fn dispatch(holder: Holder) { holder.value.submit_now(); }\n",
        ),
        (
            "server.rs",
            "pub mod receiver { pub mod job { pub struct InboundJob; } }\n",
        ),
    ]);
    assert_controller_violation(fixture.path(), "cross-module generic definition default");
}
