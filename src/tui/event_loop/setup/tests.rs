use std::sync::Arc;

use crate::access::AccessMode;
use crate::agent::{AgentKind, AgentSession, LaunchRequest, SessionPlan};
use crate::workspace::{
    CommandContext, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
};

use crate::tui::runtime::terminal::restore_after_event_loop;
use crate::tui::runtime::{
    acquire_singleton_then_refresh, load_startup_config, periodic_pull_enabled, startup_sync_plan,
};

#[test]
fn event_loop_error_still_runs_terminal_restoration() {
    let mut restored = false;

    let result = restore_after_event_loop(Err(anyhow::anyhow!("event loop failed")), || {
        restored = true;
        Ok(())
    });

    assert!(restored);
    assert_eq!(result.unwrap_err().to_string(), "event loop failed");
}

#[test]
fn required_terminal_restoration_error_supersedes_event_loop_error() {
    let result = restore_after_event_loop(Err(anyhow::anyhow!("event loop failed")), || {
        Err(anyhow::anyhow!("required restoration failed"))
    });

    assert_eq!(
        result.unwrap_err().to_string(),
        "required restoration failed"
    );
}

#[test]
fn periodic_pull_runs_only_for_a_sync_configured_shell() {
    assert!(periodic_pull_enabled(true));
    assert!(!periodic_pull_enabled(false));
}

#[test]
fn suppressed_startup_alert_still_waits_to_refresh_synced_state() {
    let plan = startup_sync_plan(true, true);

    assert!(plan.launch_sync);
    assert!(plan.arm_refresh);
    assert!(!plan.check_now);
}

#[test]
fn a_configured_sync_no_longer_delays_the_daily_triage_nudge() {
    // The nudge used to wait for the startup pull, so on a slow sync it appeared
    // seconds into a session the user had already started working in.
    let plan = startup_sync_plan(true, false);

    assert!(plan.check_now, "the nudge must not wait for the sync");
    assert!(
        plan.arm_refresh,
        "the refresh still has to run so a stale nudge can be withdrawn"
    );
    assert!(plan.launch_sync);
}

#[test]
fn an_unsynced_workspace_still_checks_immediately() {
    let plan = startup_sync_plan(false, false);

    assert!(plan.check_now);
    assert!(!plan.launch_sync);
    assert!(!plan.arm_refresh);
}

#[test]
fn held_workspace_singleton_prevents_hook_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("brain");
    std::fs::create_dir_all(&root).unwrap();
    let workspace = crate::workspace::WorkspaceContext::new(
        temp.path(),
        crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        crate::workspace::WorkspaceName::parse("brain").unwrap(),
        &root,
        "pablo",
        temp.path(),
    )
    .unwrap();
    let _held = crate::tui::singleton::Guard::acquire(&workspace).unwrap();
    let marker = temp.path().join("refresh-ran");

    let result = acquire_singleton_then_refresh(&workspace, |_| {
        std::fs::write(&marker, b"ran")?;
        Ok(())
    });

    assert!(result.is_err());
    assert!(!marker.exists());
}

#[test]
fn unrestricted_startup_ignores_malformed_unused_capability_fields_for_every_frontend() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("brain");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        r#"{
                "access_mode": "unrestricted",
                "allowed_mcps": "malformed",
                "allowed_skills": {"malformed": true},
                "enable_triage_habits": true
            }"#,
    )
    .unwrap();
    let workspace = Arc::new(
        WorkspaceContext::new(
            temp.path(),
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
            WorkspaceName::parse("brain").unwrap(),
            &root,
            "pablo",
            temp.path(),
        )
        .unwrap(),
    );

    let config = load_startup_config(&workspace).expect("unrestricted startup config");

    assert_eq!(config.access_mode, AccessMode::Unrestricted);
    let command = CommandContext::for_test(
        Arc::clone(&workspace),
        RegistryStore::from_path(temp.path().join("env.json")),
        "pablo",
    );
    for kind in AgentKind::ALL {
        let frontend = crate::agent::configured_frontend(&command, kind);
        let request = LaunchRequest::from_trusted_context(
            Arc::clone(&workspace),
            crate::actor::test_actor("pablo"),
            SessionPlan::fresh(AgentSession::new("session-1").unwrap()),
            None,
            config.access_mode,
        );
        frontend
            .launch_spec(&request)
            .expect("unrestricted frontend startup spec");
    }
}

#[test]
fn startup_remains_strict_for_access_mode_workspace_capabilities_and_live_fields() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("brain");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    let workspace = WorkspaceContext::new(
        temp.path(),
        WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        WorkspaceName::parse("brain").unwrap(),
        &root,
        "pablo",
        temp.path(),
    )
    .unwrap();

    for body in [
        r#"{"access_mode":"invalid"}"#,
        r#"{"access_mode":"workspace_only","allowed_mcps":"malformed"}"#,
        r#"{"access_mode":"unrestricted","enable_triage_habits":"malformed"}"#,
    ] {
        std::fs::write(root.join(".config/config.json"), body).unwrap();
        assert!(load_startup_config(&workspace).is_err(), "accepted {body}");
    }
}
