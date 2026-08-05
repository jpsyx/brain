use std::time::Duration;

use super::receiver_workspace_support::DualWorkspaceReceiverFixture;

#[test]
fn two_fake_tuis_share_one_process_then_orderly_close_to_unavailable_and_shutdown() {
    let mut fixture = DualWorkspaceReceiverFixture::start();
    let initial = fixture.server_snapshot();
    assert_eq!(initial.live_leases, 2);

    let personal_response = fixture.post_personal_async("SM-e2e-personal", "personal exact");
    let family_response = fixture.post_family_async("SM-e2e-family", "family exact");
    let (personal_jobs, family_jobs) = fixture.poll_both_jobs();
    assert!(
        personal_response
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    assert!(
        family_response
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    assert!(exact_lifecycle_routes(
        &personal_jobs,
        &family_jobs,
        fixture.personal.id(),
        fixture.family.id(),
        &["personal exact"],
        &["family exact"],
    ));
    let mut cross_route_mutation = personal_jobs;
    cross_route_mutation.push(family_jobs[0].clone());
    assert!(!exact_lifecycle_routes(
        &cross_route_mutation,
        &family_jobs,
        fixture.personal.id(),
        fixture.family.id(),
        &["personal exact"],
        &["family exact"],
    ));

    fixture.close_family_tui();
    let after_family = fixture.server_snapshot();
    assert_eq!(after_family.generation, initial.generation);
    assert_eq!(after_family.live_leases, 1);
    let unavailable = fixture.post_family("SM-e2e-family-closed", "discard exactly once");
    assert!(unavailable.starts_with("HTTP/1.1 200"), "{unavailable}");
    assert_eq!(unavailable.matches("Brain is unavailable").count(), 1);
    let personal_jobs = fixture.personal_jobs();
    let family_jobs = fixture.family_jobs();
    assert!(exact_lifecycle_routes(
        &personal_jobs,
        &family_jobs,
        fixture.personal.id(),
        fixture.family.id(),
        &["personal exact"],
        &["family exact"],
    ));
    assert!(fixture.server_is_running());

    let personal_continuity =
        fixture.post_personal_async("SM-e2e-personal-live", "personal still live");
    let personal_jobs = fixture.poll_personal_jobs(2);
    assert!(
        personal_continuity
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    let family_jobs = fixture.family_jobs();
    assert!(exact_lifecycle_routes(
        &personal_jobs,
        &family_jobs,
        fixture.personal.id(),
        fixture.family.id(),
        &["personal exact", "personal still live"],
        &["family exact"],
    ));

    fixture.close_personal_tui();
    fixture.wait_for_server_exit();
    assert!(!fixture.server_is_running());
    assert!(!fixture.server_state_exists());
}

fn exact_lifecycle_routes(
    personal_jobs: &[brain::server::receiver::InboundJob],
    family_jobs: &[brain::server::receiver::InboundJob],
    personal_id: brain::workspace::WorkspaceId,
    family_id: brain::workspace::WorkspaceId,
    personal_prompts: &[&str],
    family_prompts: &[&str],
) -> bool {
    personal_jobs.len() == personal_prompts.len()
        && family_jobs.len() == family_prompts.len()
        && personal_jobs
            .iter()
            .zip(personal_prompts)
            .all(|(job, prompt)| job.workspace_id == personal_id && job.prompt == *prompt)
        && family_jobs
            .iter()
            .zip(family_prompts)
            .all(|(job, prompt)| job.workspace_id == family_id && job.prompt == *prompt)
}
