use brain::users::UsersStore;

#[path = "receiver_setup_workspace/support.rs"]
mod support;

use support::Fixture;

fn run_with_log(fixture: &Fixture, args: &[&str]) -> (std::process::Output, std::path::PathBuf) {
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(args)
        .env("HOME", fixture.home.path())
        .env("XDG_CONFIG_HOME", fixture.config_home.path())
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let output = child.wait_with_output().unwrap();
    let suffix = format!("-{pid}.log");
    let path = std::fs::read_dir("/tmp")
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(&suffix))
        .max_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        })
        .expect("run log for subprocess PID");
    (output, path)
}

#[cfg(unix)]
#[test]
fn setup_private_argv_values_never_reach_run_logs_or_verbose_output() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::new();
    assert_ne!(fixture.personal.id(), fixture.family.id());
    let (ordinary, ordinary_log) = run_with_log(
        &fixture,
        &[
            "-b",
            "personal",
            "receiver",
            "setup",
            "--channels",
            "sms",
            "--public-url",
            "https://private.example.test",
            "--twilio-account-sid",
            "AC_PRIVATE_SENTINEL",
            "--twilio-auth-token",
            "TOKEN_PRIVATE_SENTINEL",
            "--twilio-from-number",
            "+12125550100",
            "--user-id",
            "pablo",
            "--phone",
            "+12125550101",
            "--phone-allowed",
            "true",
        ],
    );
    assert!(
        ordinary.status.success(),
        "{}",
        String::from_utf8_lossy(&ordinary.stderr)
    );

    let (verbose, verbose_log) = run_with_log(
        &fixture,
        &[
            "--verbose",
            "-b",
            "family",
            "receiver",
            "setup",
            "--channels=email",
            "--public-url=https://mail.example.test",
            "--resend-sending-api-key=API_PRIVATE_SENTINEL",
            "--resend-full-access-api-key=FULL_ACCESS_PRIVATE_SENTINEL",
            "--resend-from-email=sender-private@example.test",
            "--resend-webhook-signing-secret=SIGN_PRIVATE_SENTINEL",
            "--user-id=casey",
            "--email=actor-private@example.test",
            "--email-allowed=true",
            "--response-email=response-private@example.test",
        ],
    );
    assert!(
        verbose.status.success(),
        "{}",
        String::from_utf8_lossy(&verbose.stderr)
    );
    let (set, set_log) = run_with_log(
        &fixture,
        &[
            "--verbose",
            "-b",
            "personal",
            "receiver",
            "set",
            "twilio_auth_token=SET_PRIVATE_SENTINEL",
        ],
    );
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );

    let ordinary_text =
        String::from_utf8_lossy(&std::fs::read(&ordinary_log).unwrap()).into_owned();
    let verbose_text = format!(
        "{}{}{}{}{}{}",
        String::from_utf8_lossy(&verbose.stdout),
        String::from_utf8_lossy(&verbose.stderr),
        String::from_utf8_lossy(&std::fs::read(&verbose_log).unwrap()),
        String::from_utf8_lossy(&set.stdout),
        String::from_utf8_lossy(&set.stderr),
        String::from_utf8_lossy(&std::fs::read(&set_log).unwrap())
    );
    for private in [
        "AC_PRIVATE_SENTINEL",
        "TOKEN_PRIVATE_SENTINEL",
        "private.example.test",
        "+12125550100",
        "+12125550101",
    ] {
        assert!(
            !ordinary_text.contains(private),
            "ordinary log leaked {private}: {ordinary_text}"
        );
    }
    for private in [
        "API_PRIVATE_SENTINEL",
        "sender-private@example.test",
        "SIGN_PRIVATE_SENTINEL",
        "actor-private@example.test",
        "response-private@example.test",
        "SET_PRIVATE_SENTINEL",
    ] {
        assert!(
            !verbose_text.contains(private),
            "verbose output leaked {private}: {verbose_text}"
        );
    }
    assert_eq!(
        std::fs::metadata(ordinary_log)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(verbose_log).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(set_log).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn canonicalized_agent_capability_assignments_never_cross_binary_logging_boundary() {
    let fixture = Fixture::new();
    let whole = r#"{"mcps":{"mail":{"credentials":{"token":"UPPER_WHOLE_SECRET"}}}}"#;
    let (whole_output, whole_log) = run_with_log(
        &fixture,
        &[
            "--verbose",
            "-b",
            "personal",
            "env",
            "set",
            &format!("AGENT-CAPABILITIES={whole}"),
        ],
    );
    let (nested_output, nested_log) = run_with_log(
        &fixture,
        &[
            "--verbose",
            "-b",
            "personal",
            "env",
            "set",
            "AGENT-CAPABILITIES.MCPS.mail.CREDENTIALS.token=NESTED_CANONICAL_SECRET",
        ],
    );

    assert!(whole_output.status.success());
    assert!(nested_output.status.success());
    let observed = format!(
        "{}{}{}{}{}{}",
        String::from_utf8_lossy(&whole_output.stdout),
        String::from_utf8_lossy(&whole_output.stderr),
        String::from_utf8_lossy(&std::fs::read(whole_log).unwrap()),
        String::from_utf8_lossy(&nested_output.stdout),
        String::from_utf8_lossy(&nested_output.stderr),
        String::from_utf8_lossy(&std::fs::read(nested_log).unwrap()),
    );
    assert!(!observed.contains("UPPER_WHOLE_SECRET"), "{observed}");
    assert!(!observed.contains("NESTED_CANONICAL_SECRET"), "{observed}");
}

#[test]
fn malformed_supplied_provider_values_fail_before_any_selected_write() {
    let fixture = Fixture::new();
    for (public_url, from_number) in [
        ("http://not-https.example.test", "+12125550100"),
        ("https://brain.example.test/path", "+12125550100"),
        ("https://brain.example.test?private=query", "+12125550100"),
        ("https://brain.example.test", "private-invalid-sender"),
    ] {
        let registry_before = std::fs::read(&fixture.registry_path).unwrap();
        let users_path = UsersStore::path(&fixture.personal);
        let users_before = std::fs::read(&users_path).unwrap();
        let output = fixture.run(&[
            "-b",
            "personal",
            "receiver",
            "setup",
            "--channels",
            "sms",
            "--public-url",
            public_url,
            "--twilio-account-sid",
            "AC_PRIVATE",
            "--twilio-auth-token",
            "TOKEN_PRIVATE",
            "--twilio-from-number",
            from_number,
            "--user-id",
            "pablo",
            "--phone",
            "+12125550101",
            "--phone-allowed",
            "true",
        ]);

        assert!(!output.status.success());
        assert_eq!(
            std::fs::read(&fixture.registry_path).unwrap(),
            registry_before
        );
        assert_eq!(std::fs::read(&users_path).unwrap(), users_before);
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for private in [public_url, from_number, "AC_PRIVATE", "TOKEN_PRIVATE"] {
            assert!(!output.contains(private), "leaked {private}: {output}");
        }
    }
}

#[test]
fn malformed_existing_selected_provider_value_uses_the_same_no_write_validation() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry();
    let selected = registry
        .workspaces
        .get_mut(&brain::workspace::WorkspaceName::parse("personal").unwrap())
        .unwrap();
    for (name, value) in [
        (
            "brain_receiver_public_url",
            "https://brain.example.test#private-fragment",
        ),
        ("twilio_account_sid", "AC_existing"),
        ("twilio_auth_token", "TOKEN_existing"),
        ("twilio_from_number", "+12125550100"),
    ] {
        selected
            .env
            .insert(name.to_owned(), serde_json::json!(value));
    }
    brain::workspace::RegistryStore::from_path(fixture.registry_path.clone())
        .replace(&registry)
        .unwrap();
    let registry_before = std::fs::read(&fixture.registry_path).unwrap();
    let users_path = UsersStore::path(&fixture.personal);
    let users_before = std::fs::read(&users_path).unwrap();

    let output = fixture.run(&[
        "-b",
        "personal",
        "receiver",
        "setup",
        "--channels",
        "sms",
        "--user-id",
        "pablo",
        "--phone",
        "+12125550101",
        "--phone-allowed",
        "true",
    ]);

    assert!(!output.status.success());
    assert_eq!(
        std::fs::read(&fixture.registry_path).unwrap(),
        registry_before
    );
    assert_eq!(std::fs::read(users_path).unwrap(), users_before);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for private in [
        "private-fragment",
        "AC_existing",
        "TOKEN_existing",
        "+12125550100",
    ] {
        assert!(!combined.contains(private), "leaked {private}: {combined}");
    }
}
