//! `brain config` must report the receiver authorization that is actually live.
//!
//! Three declared config variables were superseded by the portable users store.
//! Reporting them from `config.json` alone made a fully configured receiver read
//! `(unset)` — indistinguishable from one nobody ever set up, and the reason a
//! completed `brain receiver setup` looked like it had silently done nothing.

#[allow(dead_code)]
#[path = "receiver_cli_support/mod.rs"]
mod support;

use support::Machine;

const PHONE: &str = "+16072809118";
const EMAIL: &str = "pablo@example.test";

/// A workspace whose receiver was configured exactly the way setup does it.
fn configured() -> Machine {
    let machine = Machine::new();
    machine.ok(&[
        "receiver",
        "setup",
        "-w",
        "family",
        "--channels",
        "both",
        "--public-url",
        "https://brain.example.test",
        "--twilio-account-sid",
        "AC123",
        "--twilio-auth-token",
        "token",
        "--twilio-from-number",
        "+12125550100",
        "--resend-sending-api-key",
        "key",
        "--resend-full-access-api-key",
        "full-access-key",
        "--resend-from-email",
        "brain@example.test",
        "--resend-webhook-signing-secret",
        "secret",
        "--user-id",
        "pablo",
        "--phone",
        PHONE,
        "--phone-allowed",
        "true",
        "--email",
        EMAIL,
        "--email-allowed",
        "true",
    ]);
    machine
}

/// The `config list` row for one variable.
fn row<'a>(listing: &'a str, name: &str) -> &'a str {
    listing
        .lines()
        .find(|line| line.starts_with(name))
        .unwrap_or_else(|| panic!("no `{name}` row in:\n{listing}"))
}

#[test]
fn config_reports_the_inbound_authorization_receiver_setup_persisted() {
    let machine = configured();

    let listing = machine.ok(&["config", "list", "-w", "family"]);

    let sms = row(&listing, "allowed_sms_senders");
    let email = row(&listing, "allowed_email_senders");
    assert!(sms.contains(PHONE), "{sms}");
    assert!(email.contains(EMAIL), "{email}");
    assert!(!sms.contains("(unset)"), "{sms}");
    assert!(!email.contains("(unset)"), "{email}");
}

#[test]
fn config_get_answers_with_the_live_value_for_a_superseded_variable() {
    let machine = configured();

    assert_eq!(
        machine.ok(&["config", "get", "allowed_sms_senders", "-w", "family"]),
        format!("{PHONE}\n")
    );
    assert_eq!(
        machine.ok(&["config", "get", "allowed_email_senders", "-w", "family"]),
        format!("{EMAIL}\n")
    );
}

#[test]
fn config_names_the_store_that_owns_those_values_and_the_command_that_edits_them() {
    let machine = configured();

    let listing = machine.ok(&["config", "list", "-w", "family"]);

    assert!(listing.contains("users.json"), "{listing}");
    assert!(listing.contains("brain user"), "{listing}");
}

#[test]
fn an_unconfigured_workspace_still_reports_those_variables_as_unset() {
    let machine = Machine::new();

    let listing = machine.ok(&["config", "list", "-w", "family"]);

    assert!(
        row(&listing, "allowed_sms_senders").contains("(unset)"),
        "{listing}"
    );
    assert!(
        row(&listing, "allowed_email_senders").contains("(unset)"),
        "{listing}"
    );
}

#[test]
fn setting_a_superseded_variable_is_refused_with_the_command_that_works() {
    let machine = configured();

    let output = machine.run(&[
        "config",
        "set",
        &format!("allowed_sms_senders={PHONE}"),
        "-w",
        "family",
    ]);

    // Writing config.json here persists a value nothing reads, which is exactly
    // the "I set it and nothing happened" the live store already caused once.
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("brain user"), "{stderr}");
    assert!(stderr.contains("users.json"), "{stderr}");
}
