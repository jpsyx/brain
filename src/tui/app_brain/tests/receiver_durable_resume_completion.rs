use super::receiver_durable_support::{
    accept_email_job_in_thread, publish_valid_completion, publish_valid_rotated_completion,
};
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn resumed_codex_run_with_the_bound_native_id_completes() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_email_job_in_thread(&app, &db, "codex-resume", "first", 100);
    let first_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(first_transport.transport());
    app.tick_receiver();
    let native_id = uuid::Uuid::new_v4().to_string();
    publish_valid_rotated_completion(&app, &native_id, "first response");
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(first.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );
    let sessions_dir = temporary.path().join("codex-sessions");
    let rollout = CodexRollout::create(&sessions_dir, &native_id);
    assert_eq!(
        rollout.path(),
        sessions_dir
            .join("9999/12/31")
            .join(format!("rollout-9999-12-31T00-00-00-{native_id}.jsonl"))
    );
    let sessions_override = crate::agent::override_codex_sessions_dir_for_test(&sessions_dir);
    let second = accept_email_job_in_thread(&app, &db, "codex-resume", "second", 200);
    let second_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(second_transport.transport());

    app.tick_receiver();

    let active = app
        .receiver
        .active_durable_run()
        .expect("resumed Codex run");
    assert_eq!(active.attribution.registered_session().as_str(), native_id);
    assert!(
        second_transport.launch_specs()[0]
            .command
            .contains("resume")
    );
    let completion_path = publish_valid_completion(&app, "second response");

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(second.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(second_transport.shutdowns(), 1);
    assert!(!completion_path.exists());
    drop(sessions_override);
    rollout.close();
    assert!(!sessions_dir.exists());
}

#[test]
fn resumed_opencode_run_with_the_bound_native_id_completes() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_email_job_in_thread(&app, &db, "opencode-resume", "first", 100);
    let first_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(first_transport.transport());
    app.tick_receiver();
    let native_id = "session-1";
    publish_valid_rotated_completion(&app, native_id, "first response");
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(first.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );
    let second = accept_email_job_in_thread(&app, &db, "opencode-resume", "second", 200);
    let second_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(second_transport.transport());

    app.tick_receiver();

    let active = app
        .receiver
        .active_durable_run()
        .expect("resumed OpenCode run");
    assert_eq!(active.attribution.registered_session().as_str(), native_id);
    assert!(
        second_transport.launch_specs()[0]
            .command
            .contains("--session")
    );
    let completion_path = publish_valid_completion(&app, "second response");

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(second.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(second_transport.shutdowns(), 1);
    assert!(!completion_path.exists());
}

struct CodexRollout {
    path: PathBuf,
    sessions_dir: PathBuf,
}

impl CodexRollout {
    fn create(sessions_dir: &Path, session_id: &str) -> Self {
        assert!(
            !sessions_dir.exists(),
            "isolated Codex sessions directory must start absent"
        );
        std::fs::create_dir(sessions_dir).expect("create isolated Codex sessions directory");
        std::fs::create_dir(sessions_dir.join("9999")).expect("create Codex rollout year");
        std::fs::create_dir(sessions_dir.join("9999/12")).expect("create Codex rollout month");
        std::fs::create_dir(sessions_dir.join("9999/12/31")).expect("create Codex rollout day");
        let path = sessions_dir
            .join("9999/12/31")
            .join(format!("rollout-9999-12-31T00-00-00-{session_id}.jsonl"));
        std::fs::write(&path, "{}\n").expect("write Codex rollout");
        Self {
            path,
            sessions_dir: sessions_dir.to_path_buf(),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn close(self) {
        std::fs::remove_file(self.path).expect("remove Codex rollout");
        std::fs::remove_dir(self.sessions_dir.join("9999/12/31"))
            .expect("remove Codex rollout day");
        std::fs::remove_dir(self.sessions_dir.join("9999/12")).expect("remove Codex rollout month");
        std::fs::remove_dir(self.sessions_dir.join("9999")).expect("remove Codex rollout year");
        std::fs::remove_dir(self.sessions_dir).expect("remove isolated Codex sessions directory");
    }
}
