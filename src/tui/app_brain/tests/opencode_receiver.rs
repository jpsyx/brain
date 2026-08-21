use super::*;

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
    configure_fake_twilio(&app);
    let recording = TransportRecording::default();
    app.brain_transport_override = Some(recording.transport());
    let actor = sms_actor();
    let workspace_id = app.command_context.workspace.id();
    enqueue_receiver_job(
        &mut app,
        InboundJob {
            job_id: uuid::Uuid::new_v4(),
            workspace_id,
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
        },
    );

    app.tick_receiver();
    let response_id = app
        .receiver
        .receiver_response_id()
        .map(str::to_owned)
        .expect("receiver response id");
    let response_path = app
        .command_context
        .workspace
        .paths()
        .responses_dir()
        .join(format!("{response_id}.json"));
    std::fs::create_dir_all(response_path.parent().expect("response directory"))
        .expect("create response directory");
    std::fs::write(
        &response_path,
        serde_json::json!({
            "actor_id": actor.user_id().as_str(),
            "channel": "sms",
            "message": "provider boundary response"
        })
        .to_string(),
    )
    .expect("completion artifact");

    app.tick_receiver();

    assert!(
        !response_path.exists(),
        "completion artifact must be consumed"
    );
    assert!(!app.receiver.remote_turn_in_flight());
    assert!(app.receiver.active_delivery_target().is_none());
    assert!(app.brain.is_some(), "completed receiver panel stays warm");
    let log = PathBuf::from(std::env::var_os("BRAIN_FAKE_CURL_LOG").expect("fake curl log"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !log.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(log.exists(), "provider boundary was not invoked");
}

fn configure_fake_twilio(app: &App) {
    let mut registry = RegistryStore::load_from(app.command_context.registry_store.path())
        .expect("workspace registry");
    let environment = &mut registry
        .workspaces
        .get_mut(app.command_context.workspace.name())
        .expect("selected workspace")
        .env;
    for (name, value) in [
        ("twilio_account_sid", "AC-test"),
        ("twilio_auth_token", "fake-token"),
        ("twilio_from_number", "+15550000000"),
    ] {
        environment.insert(name.to_owned(), serde_json::json!(value));
    }
    app.command_context
        .registry_store
        .replace(&registry)
        .expect("save fake provider configuration");
}
