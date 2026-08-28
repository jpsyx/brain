struct DeliveryCleanupDownFixture {
    job_id: String,
    instance: String,
    response: std::path::PathBuf,
    observation: std::path::PathBuf,
    observation_lock: std::path::PathBuf,
}

fn stage_delivery_cleanup_down(path: &std::path::Path, acknowledge: bool) -> DeliveryCleanupDownFixture {
    let fixture = super::binding::completion_fixture_in(
        Db::open_path_with_legacy_identity(
            path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state"),
        ReceiverJobState::Processing,
    );
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    if acknowledge {
        assert!(
            fixture
                .db
                .acknowledge_receiver_answer_controller_shutdown(
                    fixture.job_id,
                    fixture.token,
                    fixture.registration.instance(),
                    42,
                    1_600,
                )
                .expect("acknowledge confirmed controller exit")
        );
    }
    let cache = path.parent().expect("workspace cache directory");
    let instance = fixture.registration.instance().to_owned();
    let response = cache.join("responses").join(format!("{instance}.json"));
    let observation = cache
        .join("receiver-observations")
        .join(format!("{instance}.json"));
    let observation_lock = observation.with_extension("json.lock");
    std::fs::create_dir_all(response.parent().expect("responses directory"))
        .expect("create responses directory");
    std::fs::create_dir_all(observation.parent().expect("observations directory"))
        .expect("create observations directory");
    for artifact in [&response, &observation, &observation_lock] {
        std::fs::write(artifact, "private receiver artifact").expect("write private artifact");
    }
    DeliveryCleanupDownFixture {
        job_id: fixture.job_id.to_string(),
        instance,
        response,
        observation,
        observation_lock,
    }
}

fn delivery_cleanup_down_state(path: &std::path::Path, staged: &DeliveryCleanupDownFixture) -> (i64, i64, i64, Option<i64>) {
    let connection = rusqlite::Connection::open(path).expect("receiver state inspection");
    let version = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version");
    let cleanup_tables = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_answer_cleanups'",
            [],
            |row| row.get(0),
        )
        .expect("cleanup table count");
    let registrations = connection
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE brain_instance_id = ?1",
            [&staged.instance],
            |row| row.get(0),
        )
        .expect("registration count");
    let locked_pid = connection
        .query_row(
            "SELECT locked_pid FROM brain_sessions WHERE brain_instance_id = ?1",
            [&staged.instance],
            |row| row.get(0),
        )
        .expect("session lock");
    (version, cleanup_tables, registrations, locked_pid)
}

#[test]
fn v12_down_refuses_to_drop_unacknowledged_cleanup_authority() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let staged = stage_delivery_cleanup_down(&path, false);

    let error = super::super::schema::down_delivery_path(&path)
        .expect_err("unacknowledged controller exit must block downgrade");

    assert_eq!(delivery_cleanup_down_state(&path, &staged), (12, 1, 1, Some(42)));
    assert!(staged.response.is_file());
    assert!(staged.observation.is_file());
    assert!(staged.observation_lock.is_file());
    assert!(!error.to_string().contains("private"));
}

#[test]
fn v12_down_disposes_exact_session_and_private_artifacts_before_schema_loss() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let staged = stage_delivery_cleanup_down(&path, true);

    super::super::schema::down_delivery_path(&path).expect("drain and downgrade delivery state");

    assert_eq!(delivery_cleanup_down_state(&path, &staged), (11, 0, 0, None));
    assert!(!staged.response.exists());
    assert!(!staged.observation.exists());
    assert!(!staged.observation_lock.exists());
    let connection = rusqlite::Connection::open(&path).expect("downgraded receiver state");
    let job_state: String = connection
        .query_row(
            "SELECT state FROM receiver_jobs WHERE job_id = ?1",
            [&staged.job_id],
            |row| row.get(0),
        )
        .expect("downgraded answer-ready job");
    assert_eq!(job_state, "failed");
}

#[test]
fn v12_down_filesystem_failure_retains_authority_and_retries_idempotently() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let staged = stage_delivery_cleanup_down(&path, true);
    std::fs::remove_file(&staged.response).expect("replace response artifact");
    std::fs::create_dir(&staged.response).expect("stage undeletable response path");

    super::super::schema::down_delivery_path(&path)
        .expect_err("artifact cleanup failure must retain v12 authority");

    assert_eq!(delivery_cleanup_down_state(&path, &staged), (12, 1, 1, Some(42)));
    assert!(staged.response.is_dir());
    std::fs::remove_dir(&staged.response).expect("repair response path");

    super::super::schema::down_delivery_path(&path).expect("retry bounded cleanup and downgrade");

    assert_eq!(delivery_cleanup_down_state(&path, &staged), (11, 0, 0, None));
    assert!(!staged.observation.exists());
    assert!(!staged.observation_lock.exists());
}

#[test]
fn v12_down_does_not_let_a_finished_cleanup_delete_a_later_exact_registration() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let staged = stage_delivery_cleanup_down(&path, true);
    {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let later = db
            .accept_receiver_job(
                &receiver_job(Some("later-down-cleanup"), 200),
                &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
            )
            .expect("accept later receiver job");
        let later_token: String = db
            .conn
            .query_row(
                "SELECT job_token FROM receiver_jobs WHERE job_id = ?1",
                [later.job_id().to_string()],
                |row| row.get(0),
            )
            .expect("later job token");
        db.conn
            .execute(
                "INSERT INTO receiver_answer_cleanups
                   (job_id, job_token, workspace_id, conversation_id, brain_instance_id,
                    agent_kind, actor_id, channel, registered_session_id, actual_session_id,
                    controller_shutdown_acknowledged, session_released, artifacts_removed,
                    created_at_unix_ms, updated_at_unix_ms)
                 SELECT ?1, ?2, workspace_id, conversation_id, brain_instance_id,
                        agent_kind, actor_id, channel, registered_session_id, actual_session_id,
                        1, 0, 0, 1_700, 1_700
                 FROM receiver_answer_cleanups WHERE job_id = ?3",
                rusqlite::params![later.job_id().to_string(), later_token, staged.job_id],
            )
            .expect("stage later exact cleanup");
        db.conn
            .execute(
                "UPDATE receiver_answer_cleanups SET session_released = 1
                 WHERE job_id = ?1",
                [&staged.job_id],
            )
            .expect("finish prior session cleanup");
    }

    super::super::schema::down_delivery_path(&path)
        .expect("finished cleanup must not consume later exact registration");

    assert_eq!(delivery_cleanup_down_state(&path, &staged), (11, 0, 0, None));
}
