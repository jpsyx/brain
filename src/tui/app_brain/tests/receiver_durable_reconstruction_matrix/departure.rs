use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui::app_brain::tests) enum Departure {
    Orderly,
    Crash,
}

impl Departure {
    pub(in crate::tui::app_brain::tests) fn install_generic_controller(
        app: &mut App,
    ) -> TransportRecording {
        let transport = TransportRecording::default();
        app.brain.replace_brain_transport(transport.transport());
        assert!(
            app.open_or_focus_brain(None),
            "origin App did not launch its generic controller"
        );
        transport
    }

    pub(in crate::tui::app_brain::tests) fn leave(
        self,
        app: &mut App,
        generic: &TransportRecording,
        phase: RestartPhase,
    ) {
        if self == Self::Crash {
            return;
        }
        app.shutdown_receiver_runtime();
        assert!(
            app.receiver.active_durable_run().is_none(),
            "{phase:?} orderly receiver shutdown retained runtime authority"
        );
        assert!(
            app.shutdown_agent_controllers().is_empty(),
            "{phase:?} generic controller shutdown failed"
        );
        assert!(
            generic.shutdowns() == 1,
            "{phase:?} generic controller did not shut down exactly once"
        );
    }
}
