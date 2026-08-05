use std::process::Command;

use brain::users::{UserId, UsersStore};
use brain::workspace::WorkspaceManifest;

#[path = "receiver_setup_workspace/support.rs"]
mod support;

use support::Fixture;

#[test]
fn noninteractive_setup_silos_provider_values_users_and_ingress_urls() {
    let fixture = Fixture::new();
    let personal_manifest_before =
        std::fs::read(WorkspaceManifest::path(fixture.personal.root())).unwrap();
    let family_manifest_before =
        std::fs::read(WorkspaceManifest::path(fixture.family.root())).unwrap();
    let personal_ingress =
        WorkspaceManifest::load(fixture.personal.root(), env!("CARGO_PKG_VERSION"))
            .unwrap()
            .receiver_ingress_id();
    let family_ingress = WorkspaceManifest::load(fixture.family.root(), env!("CARGO_PKG_VERSION"))
        .unwrap()
        .receiver_ingress_id();

    let personal = fixture.run(&[
        "-b",
        "personal",
        "receiver",
        "setup",
        "--channels",
        "email",
        "--public-url",
        "https://personal.example.test/",
        "--resend-api-key",
        "re_personal_secret",
        "--resend-from-email",
        "brain@personal.example.test",
        "--resend-webhook-signing-secret",
        "whsec_personal_secret",
        "--user-id",
        "pablo",
        "--email",
        "Pablo@Example.TEST",
        "--email-allowed",
        "true",
    ]);
    assert!(
        personal.status.success(),
        "{}",
        String::from_utf8_lossy(&personal.stderr)
    );
    let family = fixture.run(&[
        "-b",
        "family",
        "receiver",
        "setup",
        "--channels",
        "sms",
        "--public-url",
        "https://family.example.test",
        "--twilio-account-sid",
        "AC_family",
        "--twilio-auth-token",
        "123456",
        "--twilio-from-number",
        "+16465550101",
        "--user-id",
        "alex-smith",
        "--user-name",
        "Alex Smith",
        "--phone",
        "646-555-0102",
        "--phone-allowed",
        "false",
    ]);
    assert!(
        family.status.success(),
        "{}",
        String::from_utf8_lossy(&family.stderr)
    );

    let personal_output = format!(
        "{}{}",
        String::from_utf8_lossy(&personal.stdout),
        String::from_utf8_lossy(&personal.stderr)
    );
    let family_output = format!(
        "{}{}",
        String::from_utf8_lossy(&family.stdout),
        String::from_utf8_lossy(&family.stderr)
    );
    assert!(personal_output.contains(&format!(
        "https://personal.example.test/w/{personal_ingress}/email"
    )));
    assert!(family_output.contains(&format!(
        "https://family.example.test/w/{family_ingress}/sms"
    )));
    for secret in ["re_personal_secret", "whsec_personal_secret", "123456"] {
        assert!(!personal_output.contains(secret));
        assert!(!family_output.contains(secret));
    }

    let registry = fixture.registry();
    let personal_env = &registry.select(Some("personal")).unwrap().record().env;
    let family_env = &registry.select(Some("family")).unwrap().record().env;
    assert_eq!(personal_env["resend_api_key"], "re_personal_secret");
    assert_eq!(
        personal_env["brain_receiver_public_url"],
        "https://personal.example.test"
    );
    assert!(personal_env.get("twilio_auth_token").is_none());
    assert_eq!(family_env["twilio_auth_token"], "123456");
    assert_eq!(
        family_env["brain_receiver_public_url"],
        "https://family.example.test"
    );
    assert!(family_env.get("resend_api_key").is_none());

    let personal_users = UsersStore::load(&fixture.personal).unwrap();
    let pablo = personal_users
        .user(&UserId::parse("pablo").unwrap())
        .unwrap();
    assert_eq!(pablo.emails[0].value, "pablo@example.test");
    assert!(pablo.emails[0].inbound_allowed);
    let family_users = UsersStore::load(&fixture.family).unwrap();
    let alex = family_users
        .user(&UserId::parse("alex-smith").unwrap())
        .unwrap();
    assert_eq!(alex.phones[0].value, "+16465550102");
    assert!(!alex.phones[0].inbound_allowed);

    assert_eq!(
        std::fs::read(WorkspaceManifest::path(fixture.personal.root())).unwrap(),
        personal_manifest_before
    );
    assert_eq!(
        std::fs::read(WorkspaceManifest::path(fixture.family.root())).unwrap(),
        family_manifest_before
    );
    assert!(
        !fixture
            .cache_home
            .path()
            .join("brain/server/process.json")
            .exists()
    );
}

#[test]
fn channel_specific_setup_requires_only_its_own_user_address() {
    let fixture = Fixture::new();
    let sms = fixture.run(&[
        "-b",
        "personal",
        "receiver",
        "setup",
        "--channels",
        "sms",
        "--public-url",
        "https://sms.example.test",
        "--twilio-account-sid",
        "AC_sms",
        "--twilio-auth-token",
        "sms-secret",
        "--twilio-from-number",
        "+12125550100",
        "--user-id",
        "pablo",
        "--phone",
        "+12125550101",
        "--phone-allowed",
        "true",
    ]);
    assert!(
        sms.status.success(),
        "{}",
        String::from_utf8_lossy(&sms.stderr)
    );

    let missing_email = fixture.run(&[
        "-b",
        "family",
        "receiver",
        "setup",
        "--channels",
        "email",
        "--public-url",
        "https://email.example.test",
        "--resend-api-key",
        "email-secret",
        "--resend-from-email",
        "brain@email.example.test",
        "--resend-webhook-signing-secret",
        "signing-secret",
        "--user-id",
        "casey",
    ]);
    assert!(!missing_email.status.success());
    assert!(
        String::from_utf8_lossy(&missing_email.stderr).contains("--email"),
        "{}",
        String::from_utf8_lossy(&missing_email.stderr)
    );
}

#[test]
fn receiver_ingress_is_created_once_and_survives_workspace_registry_changes() {
    let fixture = Fixture::new();
    let path = WorkspaceManifest::path(fixture.family.root());
    let before = std::fs::read(&path).unwrap();

    for args in [
        vec!["workspace", "rename", "family", "household"],
        vec!["workspace", "alias", "add", "household", "family"],
        vec!["workspace", "default", "household"],
    ] {
        let output = fixture.run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    let second_config = tempfile::tempdir().unwrap();
    let attached = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "attach",
            fixture.family.root().to_str().unwrap(),
        ])
        .env("HOME", fixture.home.path())
        .env("XDG_CONFIG_HOME", second_config.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        attached.status.success(),
        "{}",
        String::from_utf8_lossy(&attached.stderr)
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn setup_validates_selected_ingress_before_credentials_or_users_are_written() {
    let fixture = Fixture::new();
    let registry_before = std::fs::read(&fixture.registry_path).unwrap();
    let users_path = UsersStore::path(&fixture.personal);
    let users_before = std::fs::read(&users_path).unwrap();
    std::fs::write(
        WorkspaceManifest::path(fixture.personal.root()),
        b"{\"schema_version\":1,\"workspace_id\":\"broken\"}\n",
    )
    .unwrap();

    let output = fixture.run(&[
        "-b",
        "personal",
        "receiver",
        "setup",
        "--channels",
        "sms",
        "--public-url",
        "https://invalid.example.test",
        "--twilio-account-sid",
        "AC_invalid",
        "--twilio-auth-token",
        "must-not-persist",
        "--twilio-from-number",
        "+12125550100",
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
    assert!(!combined.contains("must-not-persist"));
}

#[test]
fn invalid_user_address_is_not_echoed_to_setup_output() {
    let fixture = Fixture::new();
    let private_address = "private-invalid-phone";
    let output = fixture.run(&[
        "-b",
        "personal",
        "receiver",
        "setup",
        "--channels",
        "sms",
        "--public-url",
        "https://redacted.example.test",
        "--twilio-account-sid",
        "AC_redacted",
        "--twilio-auth-token",
        "redacted-secret",
        "--twilio-from-number",
        "+12125550100",
        "--user-id",
        "pablo",
        "--phone",
        private_address,
        "--phone-allowed",
        "true",
    ]);

    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains(private_address), "{combined}");
    assert!(!combined.contains("redacted-secret"), "{combined}");
}
