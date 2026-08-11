//! `brain receiver url`: the informational webhook-URL surface.

#[path = "receiver_cli_support/mod.rs"]
mod support;

use support::Machine;

#[test]
fn url_prints_both_provider_webhooks_without_any_receiver_running() {
    let machine = Machine::new();
    machine.ok(&[
        "env",
        "set",
        "brain_receiver_public_url=https://brain.example.test",
    ]);

    // Receiver intent is off and no server was ever started.
    let printed = machine.ok(&["receiver", "url"]);

    let ingress = machine.ingress("brain");
    assert!(
        printed.contains(&format!("https://brain.example.test/w/{ingress}/sms")),
        "{printed}"
    );
    assert!(
        printed.contains(&format!("https://brain.example.test/w/{ingress}/email")),
        "{printed}"
    );
    assert!(printed.contains("Twilio (SMS)"), "{printed}");
    assert!(printed.contains("Resend (email)"), "{printed}");
}

#[test]
fn a_channel_flag_narrows_the_output_to_one_provider() {
    let machine = Machine::new();
    machine.ok(&[
        "env",
        "set",
        "brain_receiver_public_url=https://brain.example.test",
    ]);

    let sms = machine.ok(&["receiver", "url", "--sms"]);
    assert!(sms.contains("/sms"), "{sms}");
    assert!(!sms.contains("/email"), "{sms}");

    let email = machine.ok(&["receiver", "url", "--email"]);
    assert!(email.contains("/email"), "{email}");
    assert!(!email.contains("/sms"), "{email}");
}

#[test]
fn the_workspace_selector_picks_that_workspaces_own_ingress_and_origin() {
    let machine = Machine::new();
    machine.ok(&[
        "env",
        "set",
        "brain_receiver_public_url=https://default.example.test",
    ]);
    machine.ok(&[
        "env",
        "set",
        "-w",
        "family",
        "brain_receiver_public_url=https://family.example.test",
    ]);

    let family = machine.ok(&["receiver", "url", "-w", "family", "--sms"]);

    let family_ingress = machine.ingress("family");
    let default_ingress = machine.ingress("brain");
    assert_ne!(family_ingress, default_ingress, "fixture must differ");
    assert!(
        family.contains(&format!(
            "https://family.example.test/w/{family_ingress}/sms"
        )),
        "{family}"
    );
    assert!(!family.contains(&default_ingress), "{family}");
}

#[test]
fn a_workspace_with_no_public_url_says_which_variable_to_set() {
    let machine = Machine::new();

    let output = machine.run(&["receiver", "url", "-w", "family"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(
        stderr.contains("brain_receiver_public_url is unset for workspace family"),
        "{stderr}"
    );
    assert!(
        stderr.contains("brain receiver setup -w family"),
        "{stderr}"
    );
    assert!(
        stderr.contains("brain env set -w family brain_receiver_public_url="),
        "{stderr}"
    );
}

#[test]
fn receiver_status_reports_the_same_webhook_urls() {
    let machine = Machine::new();
    machine.ok(&[
        "env",
        "set",
        "brain_receiver_public_url=https://brain.example.test",
    ]);

    let status = machine.ok(&["receiver", "status"]);

    let ingress = machine.ingress("brain");
    assert!(status.contains("Webhook URLs"), "{status}");
    assert!(
        status.contains(&format!("https://brain.example.test/w/{ingress}/sms")),
        "{status}"
    );
}

#[test]
fn receiver_status_without_a_public_url_points_at_setup_instead_of_a_url() {
    let machine = Machine::new();

    let status = machine.ok(&["receiver", "status"]);

    assert!(status.contains("Webhook URLs"), "{status}");
    assert!(status.contains("brain receiver setup"), "{status}");
    assert!(!status.contains("/w/"), "{status}");
}
