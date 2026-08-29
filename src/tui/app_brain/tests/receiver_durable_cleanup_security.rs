#[cfg(unix)]
use std::os::unix::fs::symlink;

use super::receiver_durable_answer_cleanup::answer_fixture;
use super::receiver_durable_support::publish_valid_completion;
use super::*;

#[cfg(unix)]
#[test]
fn runtime_answer_cleanup_rejects_symlinked_artifact_ancestors_and_retries_exact_paths() {
    let (temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained behind exact cleanup authority");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    assert!(
        response.exists(),
        "fixture did not retain the response artifact"
    );

    let instance = response
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .expect("response instance");
    let responses = app.context.workspace().paths().responses_dir();
    let observations = app.context.workspace().paths().receiver_observations_dir();
    std::fs::create_dir_all(&observations).expect("observation directory");
    let observation = observations.join(format!("{instance}.json"));
    let observation_lock = observation.with_extension("json.lock");
    std::fs::write(&observation, "private observation").expect("observation artifact");
    std::fs::write(&observation_lock, "private observation lock").expect("observation lock");

    let real_responses = responses.with_extension("real");
    let real_observations = observations.with_extension("real");
    std::fs::rename(&responses, &real_responses).expect("retain exact responses directory");
    std::fs::rename(&observations, &real_observations)
        .expect("retain exact observations directory");
    let outside = temporary.path().join("outside-cleanup");
    let outside_responses = outside.join("responses");
    let outside_observations = outside.join("receiver-observations");
    std::fs::create_dir_all(&outside_responses).expect("outside responses");
    std::fs::create_dir_all(&outside_observations).expect("outside observations");
    let outside_response = outside_responses.join(format!("{instance}.json"));
    let outside_observation = outside_observations.join(format!("{instance}.json"));
    let outside_observation_lock = outside_observation.with_extension("json.lock");
    for path in [
        &outside_response,
        &outside_observation,
        &outside_observation_lock,
    ] {
        std::fs::write(path, "outside private artifact").expect("outside artifact");
    }
    symlink(&outside_responses, &responses).expect("responses ancestor symlink");
    symlink(&outside_observations, &observations).expect("observations ancestor symlink");

    app.tick_receiver();

    assert!(
        outside_response.exists()
            && outside_observation.exists()
            && outside_observation_lock.exists(),
        "runtime cleanup deleted an outside artifact through a symlinked ancestor"
    );
    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "runtime cleanup discarded authority after rejecting a symlinked ancestor"
    );

    std::fs::remove_file(&responses).expect("remove responses ancestor symlink");
    std::fs::remove_file(&observations).expect("remove observations ancestor symlink");
    std::fs::rename(&real_responses, &responses).expect("restore exact responses directory");
    std::fs::rename(&real_observations, &observations)
        .expect("restore exact observations directory");

    app.tick_receiver();

    assert!(
        !cleanup_authority_exists(&app, first.job_id()),
        "exact restored cleanup authority did not finish"
    );
    assert!(
        !response.exists(),
        "exact response artifact survived cleanup"
    );
    assert!(
        !observation.exists(),
        "exact observation artifact survived cleanup"
    );
    assert!(
        !observation_lock.exists(),
        "exact observation lock survived cleanup"
    );
}

fn cleanup_authority_exists(app: &App, job_id: crate::state::ReceiverJobId) -> bool {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for cleanup authority")
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM receiver_answer_cleanups WHERE job_id = ?1)",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("answer cleanup authority")
}
