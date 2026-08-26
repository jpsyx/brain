use super::receiver_durable_support::{ReceiverClock, publish_valid_rotated_completion};
use super::*;

use crate::state::{ReceiverConversationIdentity, ReceiverJobState};

const DELIVERY_CHILD: &str = "BRAIN_OPENCODE_RECEIVER_DELIVERY_CHILD";
const PRECEDENCE_CHILD: &str = "BRAIN_RECEIVER_DELIVERY_PRECEDENCE_CHILD";

#[test]
fn authenticated_completion_reaches_the_fake_provider_boundary() {
    if std::env::var_os(DELIVERY_CHILD).is_none() {
        let temporary = tempfile::tempdir().expect("temporary provider boundary");
        let bin = temporary.path().join("bin");
        std::fs::create_dir(&bin).expect("fake provider bin");
        let fake_source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/fake_curl.sh");
        let fake_curl = bin.join("curl");
        std::fs::copy(fake_source, &fake_curl).expect("copy fake curl");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&fake_curl, std::fs::Permissions::from_mode(0o755))
                .expect("make fake curl executable");
        }
        let log = temporary.path().join("curl-config.log");
        let inherited_path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&inherited_path));
        let path = std::env::join_paths(paths).expect("child PATH");
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "tui::app_brain::tests::opencode_receiver::authenticated_completion_reaches_the_fake_provider_boundary",
                "--nocapture",
            ])
            .env(DELIVERY_CHILD, "1")
            .env("BRAIN_FAKE_CURL_LOG", &log)
            .env("PATH", path)
            .output()
            .expect("run isolated delivery child");
        assert!(
            output.status.success(),
            "child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let config = std::fs::read_to_string(log).expect("fake provider invocation");
        assert!(config.contains("url = \"https://"));
        assert!(config.contains("/2010-04-01/Accounts/AC-test/Messages.json\""));
        assert!(config.contains("data-urlencode = \"To=+15551234567\""));
        assert!(config.contains("data-urlencode = \"Body=provider boundary response\""));
        return;
    }

    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    configure_fake_twilio(&app);
    let recording = TransportRecording::default();
    app.brain.replace_receiver_transport(recording.transport());
    let actor = sms_actor();
    let inbound = InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: app.context.workspace().id(),
        actor: actor.clone(),
        channel: Channel::Sms,
        prompt: "authenticated request".to_owned(),
        authenticated_sender: "+15551234567".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 1,
        provider_id: Some("provider-message-1".to_owned()),
        thread_participants: vec!["+15551234567".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    };
    let identity =
        ReceiverConversationIdentity::sms(app.context.workspace().id(), actor.user_id().clone());
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job");

    app.tick_receiver();
    let response_path = publish_valid_rotated_completion(
        &app,
        "opencode-provider-native",
        "provider boundary response",
    );

    app.tick_receiver();
    crate::server::delivery::wait_for_background_delivery();

    assert!(
        !response_path.exists(),
        "completion artifact must be consumed"
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    let log = PathBuf::from(std::env::var_os("BRAIN_FAKE_CURL_LOG").expect("fake curl log"));
    assert!(log.exists(), "provider boundary was not invoked");
}

#[test]
fn artifact_precedence_delivers_the_exact_body_once_and_lifecycle_only_delivers_nothing() {
    if std::env::var_os(PRECEDENCE_CHILD).is_none() {
        let temporary = tempfile::tempdir().expect("temporary provider boundary");
        let bin = temporary.path().join("bin");
        std::fs::create_dir(&bin).expect("fake provider bin");
        let fake_source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/fake_curl.sh");
        let fake_curl = bin.join("curl");
        std::fs::copy(fake_source, &fake_curl).expect("copy fake curl");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&fake_curl, std::fs::Permissions::from_mode(0o755))
                .expect("make fake curl executable");
        }
        let log = temporary.path().join("curl-config.log");
        let count = temporary.path().join("curl-count.log");
        let inherited_path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&inherited_path));
        let path = std::env::join_paths(paths).expect("child PATH");
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "tui::app_brain::tests::opencode_receiver::artifact_precedence_delivers_the_exact_body_once_and_lifecycle_only_delivers_nothing",
                "--nocapture",
            ])
            .env(PRECEDENCE_CHILD, "1")
            .env("BRAIN_FAKE_CURL_LOG", &log)
            .env("BRAIN_FAKE_CURL_COUNT_LOG", &count)
            .env("PATH", path)
            .output()
            .expect("run isolated delivery child");
        assert!(
            output.status.success(),
            "child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let config = std::fs::read_to_string(log).expect("fake provider invocation");
        assert!(
            config.contains("data-urlencode = \"Body=exact artifact body\""),
            "{config}"
        );
        assert_eq!(
            std::fs::read_to_string(count)
                .expect("fake provider invocation count")
                .lines()
                .count(),
            1,
            "artifact and lifecycle evidence in one tick must deliver once, while lifecycle-only completion must not deliver"
        );
        return;
    }

    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    configure_fake_twilio(&app);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_sms_job(&app, &db, "artifact and lifecycle", 100);
    let first_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(first_transport.transport());
    app.tick_receiver();
    let completion_path =
        publish_valid_rotated_completion(&app, "session-1", "exact artifact body");
    let producer_completed_at = clock.unix_ms() + 60_000;
    write_completed_snapshot(&app, "session-1", producer_completed_at);

    app.tick_receiver();
    crate::server::delivery::wait_for_background_delivery();

    let completed = db.receiver_job(first.job_id()).unwrap().unwrap();
    assert_eq!(completed.state(), ReceiverJobState::Done);
    assert_eq!(
        completed.completed_at_unix_ms(),
        Some(producer_completed_at),
        "producer evidence time must remain independent of fresh lease authorization"
    );
    assert_eq!(first_transport.shutdowns(), 1);
    assert!(!completion_path.exists());

    let second = accept_sms_job(&app, &db, "lifecycle only", 200);
    let second_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(second_transport.transport());
    app.tick_receiver();
    assert_eq!(
        app.receiver
            .active_durable_run()
            .expect("resumed receiver")
            .attribution
            .registered_session()
            .as_str(),
        "session-1"
    );
    write_completed_snapshot(&app, "session-1", 1_300);

    app.tick_receiver();
    crate::server::delivery::wait_for_background_delivery();

    assert_eq!(
        db.receiver_job(second.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    assert_eq!(second_transport.shutdowns(), 1);
}

fn accept_sms_job(
    app: &App,
    db: &Db,
    prompt: &str,
    received_at_unix_ms: u64,
) -> crate::state::ReceiverAcceptance {
    let actor = sms_actor();
    let mut inbound = receiver_job(app, actor.clone(), Channel::Sms, prompt);
    inbound.job_id = uuid::Uuid::new_v4();
    inbound.received_at_unix_ms = received_at_unix_ms;
    inbound.provider_id = Some(format!("provider-{}", inbound.job_id));
    let identity =
        ReceiverConversationIdentity::sms(app.context.workspace().id(), actor.user_id().clone());
    db.accept_receiver_job(&inbound, &identity)
        .expect("accept SMS receiver job")
}

fn write_completed_snapshot(app: &App, session_id: &str, completed_at_unix_ms: u64) {
    let active = app.receiver.active_durable_run().expect("active receiver");
    let instance = active.attribution.instance();
    let path = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    std::fs::create_dir_all(path.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(
        &path,
        serde_json::json!({
            "version": 1,
            "revision": 1,
            "phase": "completed",
            "job_token": active.claim.job().token().to_string(),
            "instance_id": instance,
            "session_id": session_id,
            "turn_id": null,
            "accepted_at_unix_ms": null,
            "progressing_at_unix_ms": null,
            "completed_at_unix_ms": completed_at_unix_ms,
        })
        .to_string(),
    )
    .expect("completed observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only observation");
    }
}

fn configure_fake_twilio(app: &App) {
    let mut registry = RegistryStore::load_from(app.context.command().registry_store.path())
        .expect("workspace registry");
    let environment = &mut registry
        .workspaces
        .get_mut(app.context.workspace().name())
        .expect("selected workspace")
        .env;
    for (name, value) in [
        ("twilio_account_sid", "AC-test"),
        ("twilio_auth_token", "fake-token"),
        ("twilio_from_number", "+15550000000"),
    ] {
        environment.insert(name.to_owned(), serde_json::json!(value));
    }
    app.context
        .command()
        .registry_store
        .replace(&registry)
        .expect("save fake provider configuration");
}
