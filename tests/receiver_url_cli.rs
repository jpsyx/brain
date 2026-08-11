//! `brain receiver url`: the informational webhook-URL surface.
//!
//! One machine serves one URL per channel. Nothing in a URL names a workspace,
//! so these outputs are machine-wide and `-w` cannot change them.

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

    assert!(
        printed.contains("https://brain.example.test/sms"),
        "{printed}"
    );
    assert!(
        printed.contains("https://brain.example.test/email"),
        "{printed}"
    );
    assert!(printed.contains("Twilio (SMS)"), "{printed}");
    assert!(printed.contains("Resend (email)"), "{printed}");
    // The URL a portal signs must be pasteable as printed, with no ingress in it.
    assert!(!printed.contains("/w/"), "{printed}");
    assert!(!printed.contains(&machine.ingress("brain")), "{printed}");
    // And it must say why one URL can serve every workspace on the machine.
    assert!(
        printed.contains("routes each message by the number"),
        "{printed}"
    );
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
fn every_workspace_selector_prints_the_same_machine_wide_url() {
    let machine = Machine::new();
    machine.ok(&[
        "env",
        "set",
        "brain_receiver_public_url=https://brain.example.test",
    ]);

    let default = machine.ok(&["receiver", "url", "--sms"]);
    let family = machine.ok(&["receiver", "url", "-w", "family", "--sms"]);

    assert_eq!(default, family);
    assert!(
        family.contains("https://brain.example.test/sms"),
        "{family}"
    );
    assert!(!family.contains(&machine.ingress("family")), "{family}");
}

#[test]
fn a_machine_with_no_public_url_says_which_variable_to_set() {
    let machine = Machine::new();

    let output = machine.run(&["receiver", "url", "-w", "family"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(
        stderr.contains("brain_receiver_public_url is unset on this machine"),
        "{stderr}"
    );
    // The exact fix is the machine-wide write, with no selector.
    assert!(
        stderr.contains("brain env set brain_receiver_public_url="),
        "{stderr}"
    );
    // Guided setup keeps the selector: it also collects this workspace's
    // credentials, and the caller asked about `family`, not the default.
    assert!(
        stderr.contains("brain receiver setup -w family"),
        "{stderr}"
    );
}

#[test]
fn setting_the_origin_for_one_workspace_sets_it_for_the_machine() {
    // `-w` is accepted everywhere, so a machine-global write has to say plainly
    // that it landed once, and every workspace must then read the same value.
    let machine = Machine::new();

    let confirmation = machine.ok(&[
        "env",
        "set",
        "-w",
        "family",
        "brain_receiver_public_url=https://brain.example.test",
    ]);

    assert!(confirmation.contains("machine-global"), "{confirmation}");
    let default = machine.ok(&["receiver", "url", "--sms"]);
    assert!(
        default.contains("https://brain.example.test/sms"),
        "{default}"
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

    assert!(status.contains("Webhook URLs"), "{status}");
    assert!(
        status.contains("https://brain.example.test/sms"),
        "{status}"
    );
}

#[test]
fn receiver_status_without_a_public_url_names_the_variable_instead_of_a_url() {
    let machine = Machine::new();

    let status = machine.ok(&["receiver", "status"]);

    assert!(status.contains("Webhook URLs"), "{status}");
    assert!(
        status.contains("brain env set brain_receiver_public_url="),
        "{status}"
    );
    assert!(!status.contains("/sms"), "{status}");
}
