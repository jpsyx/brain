#[test]
fn dead_lock_reaping_does_not_acknowledge_a_replacement_session_owner() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let workspace = receiver_workspace_id().to_string();
    let actor = receiver_user_id();
    let fixture = super::binding::completion_fixture_in(
        Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
            .expect("open receiver state"),
        ReceiverJobState::Processing,
    );
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    let job_id = fixture.job_id;
    let original_session = fixture.completed_session.as_str().to_owned();
    drop(fixture);

    let sampled = std::sync::Arc::new(std::sync::Barrier::new(2));
    let replacement_committed = std::sync::Arc::new(std::sync::Barrier::new(2));
    let reaper_path = path.clone();
    let reaper_workspace = workspace.clone();
    let reaper_actor = actor.clone();
    let sampled_by_reaper = std::sync::Arc::clone(&sampled);
    let replacement_seen_by_reaper = std::sync::Arc::clone(&replacement_committed);
    let reaper = std::thread::spawn(move || {
        Db::open_path_with_legacy_identity(
            &reaper_path,
            &reaper_workspace,
            reaper_actor.as_str(),
        )
        .expect("open reaper connection")
        .with_pid_alive(|_| false)
        .reap_dead_locks_after_sample_for_test(|| {
            sampled_by_reaper.wait();
            replacement_seen_by_reaper.wait();
        })
        .expect("reap sampled dead locks");
    });

    sampled.wait();
    let replacement = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("open replacement connection");
    let changed = replacement
        .conn
        .execute(
            "UPDATE brain_sessions
             SET locked_pid = 99, brain_instance_id = 'replacement-instance'
             WHERE agent_session_id = ?1 AND locked_pid = 42
               AND brain_instance_id = 'completion-instance'",
            [&original_session],
        )
        .expect("replace sampled session owner");
    assert_eq!(changed, 1);
    replacement_committed.wait();
    reaper.join().expect("reaper thread");

    let state: (i64, i64, String) = replacement
        .conn
        .query_row(
            "SELECT
               (SELECT controller_shutdown_acknowledged
                FROM receiver_answer_cleanups WHERE job_id = ?1),
               locked_pid, brain_instance_id
             FROM brain_sessions WHERE agent_session_id = ?2",
            rusqlite::params![job_id.to_string(), original_session],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load replacement and cleanup fence");
    assert_eq!(state.0, 0);
    assert_eq!(state.1, 99);
    assert!(
        state.2 == "replacement-instance",
        "replacement session owner changed"
    );
}
