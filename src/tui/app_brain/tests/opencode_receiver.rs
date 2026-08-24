use super::receiver_durable_support::publish_valid_completion;
use super::*;

use crate::state::{ReceiverConversationIdentity, ReceiverJobState};

const DELIVERY_CHILD: &str = "BRAIN_OPENCODE_RECEIVER_DELIVERY_CHILD";

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
        assert!(config.contains(
            "url = \"https://api.twilio.com/2010-04-01/Accounts/AC-test/Messages.json\""
        ));
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
    let response_path = publish_valid_completion(&app, "provider boundary response");

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
