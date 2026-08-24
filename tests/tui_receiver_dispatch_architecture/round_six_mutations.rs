use super::{analysis, fixture_receiver_violations, rust_fixture};

#[test]
fn receiver_local_agent_controller_name_is_not_the_brain_controller() {
    let fixture = rust_fixture(&[(
        "lib.rs",
        "mod receiver {
             pub struct AgentController;
             impl AgentController { pub fn submit_now(&mut self) {} }
             pub fn dispatch(controller: &mut AgentController) { controller.submit_now(); }
         }\n",
    )]);

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a receiver-local same-named controller is not Brain's AgentController"
    );
}

#[test]
fn picker_app_name_is_not_the_tui_app_or_receiver_consumer() {
    let fixture = rust_fixture(&[(
        "lib.rs",
        "mod picker {
             pub struct App;
             impl App {
                 pub fn open_or_focus_brain(&mut self) {}
                 pub fn tick_receiver(&mut self) {}
             }
         }
         mod receiver {
             use crate::picker::App as Picker;
             pub fn dispatch(app: &mut Picker) {
                 Picker::open_or_focus_brain(app);
                 Picker::tick_receiver(app);
             }
         }\n",
    )]);

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "an unrelated App must not acquire TUI main-panel operations"
    );
    assert_eq!(
        analysis::receiver_tick_call_count(fixture.path()),
        0,
        "an unrelated App::tick_receiver call is not the durable consumer"
    );
}

#[test]
fn receiver_local_brain_panel_name_is_not_tui_panel_state() {
    let fixture = rust_fixture(&[(
        "lib.rs",
        "mod receiver {
             pub struct BrainPanelState;
             impl BrainPanelState { pub fn take_main(&mut self) {} }
             pub fn dispatch(panel: &mut BrainPanelState) { panel.take_main(); }
         }\n",
    )]);

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a receiver-local same-named state is not the TUI brain panel"
    );
}

#[test]
fn receiver_local_inbound_job_name_does_not_mark_a_channel() {
    let fixture = rust_fixture(&[(
        "lib.rs",
        "mod receiver {
             use std::sync::mpsc::Receiver as Inbox;
             pub struct InboundJob;
             pub fn dispatch(inbox: Inbox<InboundJob>) { let _ = inbox.recv(); }
         }\n",
    )]);

    assert!(
        fixture_receiver_violations(fixture.path()).is_empty(),
        "a channel of a receiver-local same-named value is not Brain's inbound job queue"
    );
}

#[test]
fn canonical_brain_controller_reexport_and_alias_remain_guarded() {
    let fixture = rust_fixture(&[(
        "lib.rs",
        "mod agent {
             mod controller {
                 pub struct AgentController;
                 impl AgentController { pub fn submit_now(&mut self) {} }
             }
             pub use controller::AgentController;
         }
         mod receiver {
             use crate::agent::AgentController as Frontend;
             pub fn dispatch(controller: &mut Frontend) {
                 Frontend::submit_now(controller);
             }
         }\n",
    )]);

    assert!(
        fixture_receiver_violations(fixture.path())
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "the canonical Brain controller remains guarded through a re-export and alias"
    );
}

#[test]
fn canonical_tui_app_qualified_tick_remains_the_consumer() {
    let fixture = rust_fixture(&[(
        "lib.rs",
        "mod tui {
             pub struct App;
             impl App { pub fn tick_receiver(&mut self) {} }
         }
         mod receiver {
             pub fn dispatch(app: &mut crate::tui::App) {
                 crate::tui::App::tick_receiver(app);
             }
         }\n",
    )]);

    assert_eq!(
        analysis::receiver_tick_call_count(fixture.path()),
        1,
        "qualified canonical TUI App dispatch remains the durable consumer"
    );
}

#[test]
fn canonical_brain_panel_reexport_and_alias_remain_guarded() {
    let fixture = rust_fixture(&[(
        "lib.rs",
        "mod tui {
             mod state {
                 mod brain {
                     pub struct BrainPanelState;
                     impl BrainPanelState { pub fn take_main(&mut self) {} }
                 }
                 pub use brain::BrainPanelState;
             }
         }
         mod receiver {
             use crate::tui::state::BrainPanelState as Panel;
             pub fn dispatch(panel: &mut Panel) { Panel::take_main(panel); }
         }\n",
    )]);

    assert!(
        fixture_receiver_violations(fixture.path())
            .iter()
            .any(|violation| violation.contains("main-panel controller access")),
        "the canonical BrainPanelState remains guarded through a re-export and alias"
    );
}

#[test]
fn canonical_inbound_job_reexport_and_alias_remain_guarded() {
    let fixture = rust_fixture(&[(
        "lib.rs",
        "mod server {
             mod receiver {
                 mod job { pub struct InboundJob; }
                 pub use job::InboundJob;
             }
         }
         mod receiver {
             use crate::server::receiver::InboundJob as Work;
             use std::sync::mpsc::Receiver as Inbox;
             pub fn dispatch(inbox: Inbox<Work>) { let _ = inbox.recv(); }
         }\n",
    )]);

    assert!(
        fixture_receiver_violations(fixture.path())
            .iter()
            .any(|violation| violation.contains("receiver channel consume")),
        "the canonical InboundJob remains guarded through a re-export and alias"
    );
}
