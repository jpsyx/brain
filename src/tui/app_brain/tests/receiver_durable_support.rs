use super::*;

use crate::state::{EmailLineage, ReceiverAcceptance, ReceiverConversationIdentity};

#[derive(Clone)]
pub(super) struct ReceiverClock {
    state: Arc<Mutex<(std::time::Instant, chrono::DateTime<chrono::Utc>)>>,
}

impl ReceiverClock {
    pub(super) fn new() -> Self {
        Self::at_unix_ms(
            chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 8, 24, 12, 0, 0)
                .unwrap()
                .timestamp_millis()
                .try_into()
                .expect("test clock timestamp"),
        )
    }

    pub(super) fn at_unix_ms(unix_ms: u64) -> Self {
        let unix_ms = i64::try_from(unix_ms).expect("test clock timestamp fits i64");
        Self {
            state: Arc::new(Mutex::new((
                std::time::Instant::now(),
                chrono::DateTime::from_timestamp_millis(unix_ms).expect("valid test timestamp"),
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

pub(super) fn accept_email_job_in_thread(
    app: &App,
    db: &Db,
    thread: &str,
    prompt: &str,
    received_at_unix_ms: u64,
) -> ReceiverAcceptance {
    let mut inbound = receiver_job(app, email_actor(), Channel::Email, prompt);
    inbound.job_id = uuid::Uuid::new_v4();
    inbound.received_at_unix_ms = received_at_unix_ms;
    inbound.provider_id = Some(format!("provider-{}", inbound.job_id));
    inbound.authenticated_sender = "member@example.test".to_owned();
    inbound.thread_participants = vec!["member@example.test".to_owned()];
    let identity = ReceiverConversationIdentity::email(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
        EmailLineage::verified(thread).expect("verified lineage"),
    );
    db.accept_receiver_job(&inbound, &identity)
        .expect("accept durable thread job")
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
    write_completion_artifact(
        app,
        &active.attribution,
        active.attribution.registered_session(),
        message,
    )
}

pub(super) fn publish_valid_rotated_completion(
    app: &App,
    native_session_id: &str,
    message: &str,
) -> std::path::PathBuf {
    let active = app
        .receiver
        .active_durable_run()
        .expect("active receiver run");
    let native_session = AgentSession::new(native_session_id).expect("rotated native session");
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("lifecycle fixture connection")
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
            rusqlite::params![native_session.as_str(), active.attribution.instance()],
        )
        .expect("simulate lifecycle native rotation");
    SessionStore::mark_completed(&app.services, &native_session, active.attribution.scope())
        .expect("mark rotated receiver session completed");
    write_completion_artifact(app, &active.attribution, &native_session, message)
}

pub(super) fn mark_receiver_session_completed(app: &App, session: &AgentSession) {
    let active = app
        .receiver
        .active_durable_run()
        .expect("active receiver run");
    SessionStore::mark_completed(&app.services, session, active.attribution.scope())
        .expect("mark lifecycle-observed receiver session completed");
}

fn write_completion_artifact(
    app: &App,
    attribution: &crate::state::ReceiverSessionAttribution,
    session: &AgentSession,
    message: &str,
) -> std::path::PathBuf {
    let path = app
        .context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{}.json", attribution.instance()));
    std::fs::create_dir_all(path.parent().unwrap()).expect("response directory");
    std::fs::write(
        &path,
        serde_json::json!({
            "session_id": session.as_str(),
            "response_id": attribution.instance(),
            "frontend": attribution.scope().agent_kind().as_str(),
            "workspace_id": attribution.scope().workspace_id().to_string(),
            "actor_id": attribution.scope().actor().user_id().as_str(),
            "channel": attribution.scope().actor().channel().as_str(),
            "completion_status": "completed",
            "job_token": active_job_token(app),
            "message": message,
        })
        .to_string(),
    )
    .expect("completion artifact");
    path
}

fn active_job_token(app: &App) -> String {
    app.receiver
        .active_durable_run()
        .expect("active receiver run")
        .claim
        .job()
        .token()
        .to_string()
}
