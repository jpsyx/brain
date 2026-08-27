use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::*;

use crate::state::{
    ReceiverConversationId, ReceiverJobId, ReceiverJobToken, ReceiverNonterminalObservationPhase,
    ReceiverObservation,
};

pub(super) struct NativeCleanupRun {
    pub(super) job_id: ReceiverJobId,
    pub(super) token: ReceiverJobToken,
    pub(super) conversation_id: ReceiverConversationId,
    pub(super) instance: String,
    pub(super) registered: AgentSession,
    pub(super) native: AgentSession,
    pub(super) scope: SessionScope,
    pub(super) state_path: PathBuf,
    pub(super) workspace_id: String,
}

pub(super) fn launch_rotated_accepted_run(
    app: &mut App,
    db: &Db,
    clock: &ReceiverClock,
    prompt: &str,
    received_at_unix_ms: u64,
) -> NativeCleanupRun {
    launch_rotated_accepted_run_with_prior(app, db, clock, prompt, received_at_unix_ms, None)
}

pub(super) fn launch_prior_bound_rotated_accepted_run(
    app: &mut App,
    db: &Db,
    clock: &ReceiverClock,
    prompt: &str,
    received_at_unix_ms: u64,
    actual_conflict: bool,
) -> NativeCleanupRun {
    launch_rotated_accepted_run_with_prior(
        app,
        db,
        clock,
        prompt,
        received_at_unix_ms,
        Some(actual_conflict),
    )
}

fn launch_rotated_accepted_run_with_prior(
    app: &mut App,
    db: &Db,
    clock: &ReceiverClock,
    prompt: &str,
    received_at_unix_ms: u64,
    prior_binding: Option<bool>,
) -> NativeCleanupRun {
    let accepted = accept_email_job(app, db, prompt, received_at_unix_ms);
    if prior_binding.is_some() {
        let prior = AgentSession::new(format!("prior-native-{}", uuid::Uuid::new_v4()))
            .expect("prior native session");
        let scope = SessionScope::new(
            AgentKind::Codex,
            app.context.workspace().id(),
            email_actor(),
        );
        SessionStore::register(db, &prior, "prior-native-instance", 41, &scope)
            .expect("register prior native session");
        SessionStore::release(db, "prior-native-instance")
            .expect("leave prior native session resumable");
        let binding = crate::state::ReceiverSessionBinding::new(AgentKind::Codex, prior.as_str())
            .expect("prior receiver binding");
        db.update_receiver_conversation(
            accepted.conversation_id(),
            "portable transcript",
            Some(&binding),
            clock.unix_ms(),
        )
        .expect("retain prior receiver binding");
    }
    app.tick_receiver();
    let active = app
        .receiver
        .active_durable_run()
        .expect("launched fresh receiver");
    let registered = active.attribution.registered_session().clone();
    assert!(registered.as_str().starts_with("pending-receiver-"));
    let native = AgentSession::new(uuid::Uuid::new_v4().to_string()).expect("native session");
    assert_ne!(registered, native);
    let lifecycle = rusqlite::Connection::open(app.context.state_db_path())
        .expect("lifecycle fixture connection");
    lifecycle
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             SELECT agent_kind, ?1, brain_instance_id, locked_pid, source,
                    workspace_id, actor_id, channel, created_at, last_active_at
             FROM brain_sessions
             WHERE workspace_id = ?2 AND brain_instance_id = ?3
               AND agent_kind = ?4 AND actor_id = ?5 AND channel = ?6
               AND agent_session_id = ?7 AND locked_pid IS NOT NULL",
            rusqlite::params![
                native.as_str(),
                app.context.workspace().id().to_string(),
                active.attribution.instance(),
                active.attribution.scope().agent_kind().as_str(),
                active.attribution.scope().actor().user_id().as_str(),
                active.attribution.scope().actor().channel().as_str(),
                registered.as_str(),
            ],
        )
        .expect("simulate exact native session rotation");
    lifecycle
        .execute(
            "UPDATE brain_sessions SET locked_pid = NULL
             WHERE workspace_id = ?1 AND brain_instance_id = ?2
               AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
               AND agent_session_id = ?6",
            rusqlite::params![
                app.context.workspace().id().to_string(),
                active.attribution.instance(),
                active.attribution.scope().agent_kind().as_str(),
                active.attribution.scope().actor().user_id().as_str(),
                active.attribution.scope().actor().channel().as_str(),
                registered.as_str(),
            ],
        )
        .expect("unlock rotated placeholder");
    if prior_binding == Some(true) {
        lifecycle
            .execute(
                "UPDATE receiver_session_registrations
                 SET actual_session_id = (
                   SELECT agent_session_id FROM receiver_conversations
                   WHERE workspace_id = receiver_session_registrations.workspace_id
                     AND conversation_id = receiver_session_registrations.conversation_id
                 )
                 WHERE workspace_id = ?1 AND conversation_id = ?2
                   AND brain_instance_id = ?3 AND registered_session_id = ?4",
                rusqlite::params![
                    app.context.workspace().id().to_string(),
                    accepted.conversation_id().to_string(),
                    active.attribution.instance(),
                    registered.as_str(),
                ],
            )
            .expect("establish conflicting registration actual session");
    }
    let job_id = accepted.job_id();
    let token = active.claim.job().token();
    let owner = active.claim.claim().owner().to_owned();
    let instance = active.attribution.instance().to_owned();
    let conversation_id = accepted.conversation_id();
    let scope = active.attribution.scope().clone();
    assert!(
        db.apply_receiver_observation(
            job_id,
            &owner,
            &ReceiverObservation {
                token,
                instance: instance.clone(),
                session_id: native.as_str().to_owned(),
                phase: ReceiverNonterminalObservationPhase::Accepted,
                revision: 1,
                observed_at_unix_ms: clock.unix_ms(),
                authorized_at_unix_ms: clock.unix_ms(),
            },
        )
        .expect("persist exact accepted observation")
    );
    NativeCleanupRun {
        job_id,
        token,
        conversation_id,
        instance,
        registered,
        native,
        scope,
        state_path: app.context.state_db_path().to_path_buf(),
        workspace_id: app.context.workspace().id().to_string(),
    }
}

pub(super) fn claim_native_session(db: &Db, run: &NativeCleanupRun, instance: &str) -> bool {
    SessionStore::claim(db, &run.native, instance, 4242, &run.scope)
        .expect("claim exact native session")
}

pub(super) fn registration_actual_session(state_path: &Path, instance: &str) -> Option<String> {
    rusqlite::Connection::open(state_path)
        .expect("registration fixture connection")
        .query_row(
            "SELECT actual_session_id FROM receiver_session_registrations
             WHERE brain_instance_id = ?1",
            [instance],
            |row| row.get(0),
        )
        .expect("load exact registration attribution")
}

pub(super) fn conversation_binding(run: &NativeCleanupRun) -> (Option<String>, Option<String>) {
    fixture_connection(run)
        .query_row(
            "SELECT agent_kind, agent_session_id FROM receiver_conversations
             WHERE workspace_id = ?1 AND conversation_id = ?2",
            rusqlite::params![&run.workspace_id, run.conversation_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load exact conversation binding")
}

pub(super) fn registration_count(run: &NativeCleanupRun) -> i64 {
    fixture_connection(run)
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND conversation_id = ?2
               AND brain_instance_id = ?3 AND registered_session_id = ?4",
            rusqlite::params![
                &run.workspace_id,
                run.conversation_id.to_string(),
                &run.instance,
                run.registered.as_str(),
            ],
            |row| row.get(0),
        )
        .expect("count exact receiver registration")
}

pub(super) fn fixture_connection(run: &NativeCleanupRun) -> rusqlite::Connection {
    rusqlite::Connection::open(&run.state_path).expect("receiver fixture connection")
}

pub(super) fn seed_receiver_artifacts(app: &App, instance: &str) -> [PathBuf; 3] {
    let response = app
        .context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{instance}.json"));
    let observation = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    let lock = observation.with_extension("json.lock");
    let paths = [response, observation, lock];
    for path in &paths {
        std::fs::create_dir_all(path.parent().expect("artifact parent"))
            .expect("artifact directory");
        std::fs::write(path, "private local artifact").expect("seed exact local artifact");
    }
    paths
}
