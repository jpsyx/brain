#[allow(dead_code, unused_imports)]
#[path = "receiver_workspace_support/mod.rs"]
mod receiver_workspace_support;

#[path = "multi_workspace_acceptance/access.rs"]
mod access;
#[path = "multi_workspace_acceptance/merge.rs"]
mod merge;
#[path = "multi_workspace_acceptance/setup.rs"]
mod setup;
#[path = "multi_workspace_acceptance/task.rs"]
mod task;

use std::time::Duration;

use brain::sync::lock;
use brain::tasks::triage_habits::{
    ManagedTriageCompletion, ManagedTriageKind, complete_managed_triage,
};
use brain::workspace::{FeatureStatus, RequirementScope, RequirementStatus, requirements};
use receiver_workspace_support::DualWorkspaceReceiverFixture;

#[test]
fn personal_and_family_workspaces_complete_the_multitenant_lifecycle() {
    let mut fixture = DualWorkspaceReceiverFixture::start();
    let scenario = setup::prepare(&fixture);

    assert_ne!(scenario.personal.id(), scenario.family.id());
    assert_ne!(
        scenario.personal.paths().cache_dir(),
        scenario.family.paths().cache_dir()
    );
    setup::assert_selector_cli(&scenario);

    assert_ne!(
        scenario.personal.paths().tui_lock(),
        scenario.family.paths().tui_lock()
    );
    assert!(scenario.personal.paths().tui_lock().exists());
    assert!(scenario.family.paths().tui_lock().exists());
    assert_eq!(fixture.server_snapshot().live_leases, 2);

    let response = fixture.post_family_async("SM-acceptance-family", "Add the grocery task");
    let family_jobs = setup::poll_family_jobs(&mut fixture, 1);
    assert!(
        response
            .recv_timeout(Duration::from_secs(2))
            .expect("family provider response")
            .starts_with("HTTP/1.1 200")
    );
    let family_job = &family_jobs[0];
    assert_eq!(family_job.workspace_id, scenario.family.id());
    assert_eq!(family_job.actor.user_id().as_str(), "wife");
    task::FakeAgentTaskTransport::new(&scenario).create_from(family_job);

    let personal_sync =
        lock::try_acquire(&scenario.personal.paths().sync_lock()).expect("personal sync lock");
    let family_sync =
        lock::try_acquire(&scenario.family.paths().sync_lock()).expect("family sync lock");
    assert!(lock::try_acquire(&scenario.family.paths().sync_lock()).is_none());
    drop((personal_sync, family_sync));

    merge::assert_independent_display_ids_converge();

    let family_config = brain::config::Config::load(&scenario.family);
    assert!(!family_config.enable_triage_habits);
    let habits_path = scenario.family.root().join("tasks/habits.csv");
    // First-run setup seeds the habits store for every workspace, so what
    // "disabled" means here is that it holds no managed rows — not that the
    // table is missing.
    assert!(
        managed_rows(&habits_path).is_empty(),
        "disabled triage has no managed history"
    );
    assert_eq!(
        complete_managed_triage(
            &scenario.family,
            ManagedTriageKind::Daily,
            family_config.enable_triage_habits,
            chrono::NaiveDate::from_ymd_opt(2026, 8, 6).expect("fixed acceptance date"),
        )
        .expect("disabled triage acknowledgement"),
        ManagedTriageCompletion::Disabled
    );
    assert!(
        managed_rows(&habits_path).is_empty(),
        "disabled triage must not create managed history"
    );
    let readiness = requirements(&setup::command_context(&scenario)).expect("family readiness");
    for scope in [
        RequirementScope::TriageHabits,
        RequirementScope::TriageModal,
    ] {
        let status = readiness
            .iter()
            .find(|requirement| requirement.scope() == &scope)
            .expect("triage readiness row")
            .status();
        assert_eq!(status, RequirementStatus::Feature(FeatureStatus::Off));
    }
    access::assert_frontend_neutral_workspace_only_launch(&scenario, &family_job.actor);

    fixture.close_family_tui();
    assert_eq!(fixture.server_snapshot().live_leases, 1);
    let unavailable = fixture.post_family("SM-acceptance-closed", "must be discarded");
    assert!(
        unavailable.contains("Brain is unavailable"),
        "{unavailable}"
    );
    assert!(fixture.server_is_running());

    let personal_response =
        fixture.post_personal_async("SM-acceptance-personal", "personal stays live");
    let personal_jobs = fixture.poll_personal_jobs(1);
    assert!(
        personal_response
            .recv_timeout(Duration::from_secs(2))
            .expect("personal provider response")
            .starts_with("HTTP/1.1 200")
    );
    assert_eq!(personal_jobs[0].workspace_id, scenario.personal.id());

    fixture.close_personal_tui();
    fixture.wait_for_server_exit();
    assert!(!fixture.server_is_running());
    assert!(!fixture.server_state_exists());
}

/// Every non-header row of a habits table that carries a managed `system_key`.
fn managed_rows(habits: &std::path::Path) -> Vec<String> {
    let Ok(body) = std::fs::read_to_string(habits) else {
        return Vec::new();
    };
    body.lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .filter(|line| {
            line.rsplit(',')
                .next()
                .is_some_and(|system_key| !system_key.trim().is_empty())
        })
        .map(str::to_owned)
        .collect()
}
