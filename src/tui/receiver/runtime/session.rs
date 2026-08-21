use crate::actor::ActorContext;

use super::ReceiverRuntime;

pub(crate) struct SessionLaunch {
    pub(crate) receiver_request: bool,
    pub(crate) requested_actor: Option<ActorContext>,
}

pub(crate) struct SessionSelection {
    pub(crate) resume_override: Option<String>,
    pub(crate) force_fresh: bool,
}

impl ReceiverRuntime {
    pub(crate) fn begin_session_launch(&mut self) -> SessionLaunch {
        let receiver_request = self.requested_actor.is_some();
        if receiver_request {
            self.receiver_response_id = None;
        } else {
            self.interactive_response_id = None;
            self.interactive_agent_session_id = None;
        }
        SessionLaunch {
            receiver_request,
            requested_actor: self.requested_actor.clone(),
        }
    }

    pub(crate) fn begin_session_selection(&mut self) -> SessionSelection {
        SessionSelection {
            resume_override: self.resume_session.clone(),
            force_fresh: std::mem::take(&mut self.force_fresh),
        }
    }

    pub(crate) fn record_session_started(
        &mut self,
        receiver_request: bool,
        response_id: String,
        agent_session_id: String,
    ) {
        self.requested_actor = None;
        self.resume_session = None;
        if receiver_request {
            self.receiver_response_id = Some(response_id);
        } else {
            self.interactive_response_id = Some(response_id);
            self.interactive_agent_session_id = Some(agent_session_id);
        }
    }

    pub(crate) fn record_session_launch_failed(&mut self, receiver_request: bool) {
        if receiver_request {
            self.receiver_response_id = None;
        } else {
            self.interactive_response_id = None;
            self.interactive_agent_session_id = None;
        }
    }

    #[must_use]
    pub(crate) fn interactive_completion_to_clear(&self) -> Option<&str> {
        self.lease
            .is_none()
            .then_some(self.interactive_response_id.as_deref())
            .flatten()
    }

    pub(crate) fn record_interactive_agent_session(&mut self, session_id: String) {
        self.interactive_agent_session_id = Some(session_id);
    }

    #[must_use]
    pub(crate) fn interactive_agent_session_to_resume(&self) -> Option<&str> {
        self.interactive_agent_session_id.as_deref()
    }

    pub(crate) fn prepare_interactive_restore(&mut self, can_resume: bool) {
        self.resume_session = can_resume
            .then(|| self.interactive_agent_session_id.take())
            .flatten();
    }

    #[cfg(test)]
    pub(crate) fn record_interactive_session(
        &mut self,
        response_id: String,
        agent_session_id: String,
    ) {
        self.interactive_response_id = Some(response_id);
        self.interactive_agent_session_id = Some(agent_session_id);
    }
}
