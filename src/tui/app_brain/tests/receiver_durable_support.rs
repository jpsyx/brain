use super::*;

use crate::state::{EmailLineage, ReceiverAcceptance, ReceiverConversationIdentity};

#[derive(Clone)]
pub(super) struct ReceiverClock {
    state: Arc<Mutex<(std::time::Instant, chrono::DateTime<chrono::Utc>)>>,
}

impl ReceiverClock {
    pub(super) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new((
                std::time::Instant::now(),
                chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 8, 24, 12, 0, 0).unwrap(),
            ))),
        }
    }

    pub(super) fn advance(&self, duration: std::time::Duration) {
        let mut state = self.state.lock().unwrap();
        state.0 += duration;
        state.1 += chrono::TimeDelta::from_std(duration).unwrap();
    }

    pub(super) fn unix_ms(&self) -> u64 {
        u64::try_from(self.state.lock().unwrap().1.timestamp_millis()).unwrap()
    }
}

impl crate::tui::app_sync::ReceiverSyncRuntime for ReceiverClock {
    fn monotonic_now(&self) -> std::time::Instant {
        self.state.lock().unwrap().0
    }

    fn utc_now(&self) -> chrono::DateTime<chrono::Utc> {
        self.state.lock().unwrap().1
    }

    fn live_sync_state(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<crate::sync::current::CurrentState> {
        None
    }

    fn latest_successful_downstream_id(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<i64> {
        None
    }

    fn latest_downstream_completion(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<String> {
        None
    }

    fn spawn_detached_sync(
        &self,
        _workspace: &WorkspaceContext,
        _direction: crate::sync::args::Direction,
    ) -> Option<u32> {
        None
    }
}

pub(super) fn accept_email_job(
    app: &App,
    db: &Db,
    prompt: &str,
    received_at_unix_ms: u64,
) -> ReceiverAcceptance {
    accept_email_job_with_id(app, db, prompt, received_at_unix_ms, uuid::Uuid::new_v4())
}

pub(super) fn accept_email_job_with_id(
    app: &App,
    db: &Db,
    prompt: &str,
    received_at_unix_ms: u64,
    job_id: uuid::Uuid,
) -> ReceiverAcceptance {
    let mut inbound = receiver_job(app, email_actor(), Channel::Email, prompt);
    inbound.job_id = job_id;
    inbound.received_at_unix_ms = received_at_unix_ms;
    inbound.provider_id = Some(format!("provider-{job_id}"));
    inbound.authenticated_sender = "member@example.test".to_owned();
    inbound.thread_participants = vec!["member@example.test".to_owned()];
    let identity = ReceiverConversationIdentity::email(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
        EmailLineage::verified(format!("thread-{received_at_unix_ms}")).unwrap(),
    );
    db.accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job")
}

pub(super) fn publish_valid_completion(app: &App, message: &str) -> std::path::PathBuf {
    let active = app
        .receiver
        .active_durable_run()
        .expect("active receiver run");
    SessionStore::mark_completed(
        &app.services,
        active.attribution.registered_session(),
        active.attribution.scope(),
    )
    .expect("mark exact receiver session completed");
    let path = app
        .context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{}.json", active.attribution.instance()));
    std::fs::create_dir_all(path.parent().unwrap()).expect("response directory");
    std::fs::write(
        &path,
        serde_json::json!({
            "session_id": active.attribution.registered_session().as_str(),
            "response_id": active.attribution.instance(),
            "frontend": active.attribution.scope().agent_kind().as_str(),
            "workspace_id": active.attribution.scope().workspace_id().to_string(),
            "actor_id": active.attribution.scope().actor().user_id().as_str(),
            "channel": active.attribution.scope().actor().channel().as_str(),
            "completion_status": "completed",
            "message": message,
        })
        .to_string(),
    )
    .expect("completion artifact");
    path
}
