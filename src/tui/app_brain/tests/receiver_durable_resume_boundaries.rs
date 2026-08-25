use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::*;

use crate::state::{ReceiverAcceptance, ReceiverJobState, ReceiverSessionBinding};
use crate::tui::receiver::ReceiverLaunchBoundary;

fn bind_native_session(
    db: &Db,
    accepted: &ReceiverAcceptance,
    kind: AgentKind,
    native_session_id: &str,
) {
    let binding =
        ReceiverSessionBinding::new(kind, native_session_id).expect("exact receiver binding");
    db.update_receiver_conversation(
        accepted.conversation_id(),
        "portable transcript",
        Some(&binding),
        101,
    )
    .expect("bind receiver conversation");
}

fn assert_claim_retained_without_retry(db: &Db, accepted: &ReceiverAcceptance) {
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
    assert_eq!(job.last_error(), None);
}

#[test]
fn missing_resume_history_cannot_fall_back_fresh_after_claim_expiry() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let clock = ReceiverClock::new();
        app.services
            .replace_receiver_sync_runtime(Box::new(clock.clone()));
        let db = Db::open(app.context.workspace()).expect("state DB");
        let accepted = accept_email_job(&app, &db, "missing native history", 100);
        let native_session_id = format!("missing-native-{}", uuid::Uuid::new_v4());
        bind_native_session(&db, &accepted, kind, &native_session_id);
        let codex_sessions = temporary.path().join("isolated-codex-sessions");
        let _codex_override = (kind == AgentKind::Codex)
            .then(|| crate::agent::override_codex_sessions_dir_for_test(&codex_sessions));
        let transport = TransportRecording::default();
        app.brain.replace_receiver_transport(transport.transport());
        app.receiver.install_launch_boundary_hook(
            ReceiverLaunchBoundary::ResumeValidation,
            Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
        );

        app.tick_receiver();

        assert!(
            transport.launch_specs().is_empty(),
            "{} must not launch after the exact claim expires",
            kind.label()
        );
        assert_claim_retained_without_retry(&db, &accepted);
    }
}

#[test]
fn missing_resume_history_falls_back_fresh_while_claim_is_live() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).expect("state DB");
        let accepted = accept_email_job(&app, &db, "missing native history", 100);
        let native_session_id = format!("missing-native-{}", uuid::Uuid::new_v4());
        bind_native_session(&db, &accepted, kind, &native_session_id);
        let codex_sessions = temporary.path().join("isolated-codex-sessions");
        let _codex_override = (kind == AgentKind::Codex)
            .then(|| crate::agent::override_codex_sessions_dir_for_test(&codex_sessions));
        let validations = Arc::new(Mutex::new(0));
        let observed_validations = Arc::clone(&validations);
        app.receiver.install_launch_boundary_hook(
            ReceiverLaunchBoundary::ResumeValidation,
            Box::new(move || *observed_validations.lock().expect("validation count") += 1),
        );
        let transport = TransportRecording::default();
        app.brain.replace_receiver_transport(transport.transport());

        app.tick_receiver();

        assert_eq!(*validations.lock().expect("validation count"), 1);
        assert_eq!(transport.launch_specs().len(), 1, "{}", kind.label());
        assert_eq!(
            db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
            ReceiverJobState::Launched,
            "{}",
            kind.label()
        );
    }
}

fn malformed_session_command(temporary: &tempfile::TempDir) -> String {
    let fake =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/fake_opencode.sh");
    let wrapper = temporary.path().join("opencode-malformed-session.sh");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nset -eu\nif [ \"$*\" = \"session list --format json\" ]; then\n  printf 'not-json\\n'\n  exit 0\nfi\nexec {} \"$@\"\n",
            crate::agent::frontend::shell_quote(&fake.display().to_string())
        ),
    )
    .expect("write malformed-session OpenCode fixture");
    format!(
        "sh {}",
        crate::agent::frontend::shell_quote(&wrapper.display().to_string())
    )
}

fn validation_error_app(
    temporary: &tempfile::TempDir,
) -> (App, Db, ReceiverAcceptance, TransportRecording) {
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app_with_agent_command(
        temporary,
        &cli,
        AgentKind::OpenCode,
        &malformed_session_command(temporary),
    );
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "unavailable native history", 100);
    bind_native_session(&db, &accepted, AgentKind::OpenCode, "session-1");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    (app, db, accepted, transport)
}

#[test]
fn resume_validation_error_cannot_fall_back_fresh_after_claim_expiry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (mut app, db, accepted, transport) = validation_error_app(&temporary);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::ResumeValidation,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );

    app.tick_receiver();

    assert!(transport.launch_specs().is_empty());
    assert_claim_retained_without_retry(&db, &accepted);
}

#[test]
fn resume_validation_error_falls_back_fresh_while_claim_is_live() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (mut app, db, accepted, transport) = validation_error_app(&temporary);
    let validations = Arc::new(Mutex::new(0));
    let observed_validations = Arc::clone(&validations);
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::ResumeValidation,
        Box::new(move || *observed_validations.lock().expect("validation count") += 1),
    );

    app.tick_receiver();

    assert_eq!(*validations.lock().expect("validation count"), 1);
    assert_eq!(transport.launch_specs().len(), 1);
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched
    );
}

fn locked_resume_app(
    temporary: &tempfile::TempDir,
) -> (App, Db, ReceiverAcceptance, TransportRecording) {
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "locked native session", 100);
    bind_native_session(&db, &accepted, AgentKind::OpenCode, "session-1");
    let scope = SessionScope::new(
        AgentKind::OpenCode,
        app.context.workspace().id(),
        email_actor(),
    );
    db.register_scoped_fresh("session-1", "other-instance", 999, &scope)
        .expect("lock exact native session elsewhere");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    (app, db, accepted, transport)
}

#[test]
fn rejected_resume_claim_cannot_fall_back_fresh_after_claim_expiry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (mut app, db, accepted, transport) = locked_resume_app(&temporary);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let fresh_registrations = Arc::new(Mutex::new(0));
    let observed_fresh = Arc::clone(&fresh_registrations);
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Registration,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Registration,
        Box::new(move || *observed_fresh.lock().expect("fresh registration count") += 1),
    );

    app.tick_receiver();

    assert_eq!(
        *fresh_registrations
            .lock()
            .expect("fresh registration count"),
        0
    );
    assert!(transport.launch_specs().is_empty());
    assert_claim_retained_without_retry(&db, &accepted);
}

#[test]
fn rejected_resume_claim_falls_back_fresh_while_claim_is_live() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (mut app, db, accepted, transport) = locked_resume_app(&temporary);
    let registrations = Arc::new(Mutex::new(0));
    for _ in 0..2 {
        let observed = Arc::clone(&registrations);
        app.receiver.install_launch_boundary_hook(
            ReceiverLaunchBoundary::Registration,
            Box::new(move || *observed.lock().expect("registration count") += 1),
        );
    }

    app.tick_receiver();

    assert_eq!(*registrations.lock().expect("registration count"), 2);
    assert_eq!(transport.launch_specs().len(), 1);
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched
    );
}
