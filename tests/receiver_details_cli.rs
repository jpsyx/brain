//! Bare `brain receiver` and the two address commands.
//!
//! These are the informational half of the receiver surface: what is this
//! machine's receiver configured as, and what address does it answer on.

#[path = "receiver_cli_support/mod.rs"]
mod support;

use support::Machine;

/// A machine whose two workspaces each have their own provider configuration.
fn configured() -> Machine {
    let machine = Machine::new();
    for (workspace, url, phone, email) in [
        (
            "brain",
            "https://brain.example.test",
            "+12125550100",
            "brain@example.test",
        ),
        (
            "family",
            "https://family.example.test",
            "+12125550199",
            "family@example.test",
        ),
    ] {
        machine.ok(&[
            "env",
            "set",
            "-w",
            workspace,
            &format!("brain_receiver_public_url={url}"),
        ]);
        machine.ok(&[
            "env",
            "set",
            "-w",
            workspace,
            &format!("twilio_from_number={phone}"),
        ]);
        machine.ok(&[
            "env",
            "set",
            "-w",
            workspace,
            &format!("resend_from_email={email}"),
        ]);
    }
    machine
}

#[test]
fn bare_receiver_reports_every_registered_workspace() {
    let machine = configured();

    let listing = machine.ok(&["receiver"]);

    for workspace in ["brain", "family"] {
        assert!(
            listing.contains(&format!("Receiver details  {workspace}")),
            "{listing}"
        );
    }
    assert!(listing.contains("brain@example.test"), "{listing}");
    assert!(listing.contains("family@example.test"), "{listing}");
    assert!(listing.contains("+12125550100"), "{listing}");
    assert!(listing.contains("+12125550199"), "{listing}");
    assert!(
        listing.contains(&format!(
            "https://family.example.test/w/{}/sms",
            machine.ingress("family")
        )),
        "{listing}"
    );
}

#[test]
fn the_workspace_selector_narrows_the_listing_to_that_workspace_alone() {
    let machine = configured();

    let listing = machine.ok(&["receiver", "-w", "family"]);

    assert!(listing.contains("Receiver details  family"), "{listing}");
    assert!(!listing.contains("Receiver details  brain"), "{listing}");
    assert!(!listing.contains("brain@example.test"), "{listing}");
}

#[test]
fn each_address_command_prints_the_bare_configured_value() {
    let machine = configured();

    // Bare and unstyled, so the answer pipes straight into another command.
    assert_eq!(machine.ok(&["receiver", "email"]), "brain@example.test\n");
    assert_eq!(machine.ok(&["receiver", "phone"]), "+12125550100\n");
    assert_eq!(
        machine.ok(&["receiver", "email", "-w", "family"]),
        "family@example.test\n"
    );
    assert_eq!(
        machine.ok(&["receiver", "phone", "-w", "family"]),
        "+12125550199\n"
    );
}

#[test]
fn an_unconfigured_address_names_the_variable_and_both_ways_to_set_it() {
    let machine = Machine::new();

    let output = machine.run(&["receiver", "email", "-w", "family"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(
        stderr.contains("resend_from_email is unset for workspace family"),
        "{stderr}"
    );
    assert!(
        stderr.contains("brain receiver setup -w family"),
        "{stderr}"
    );
    assert!(
        stderr.contains("brain env set -w family resend_from_email="),
        "{stderr}"
    );
}

#[test]
fn an_unconfigured_workspace_reads_as_not_set_rather_than_failing_the_listing() {
    let machine = Machine::new();

    let listing = machine.ok(&["receiver"]);

    assert!(listing.contains("Receiver details  brain"), "{listing}");
    assert!(listing.contains("Receiver details  family"), "{listing}");
    assert!(listing.contains("not set"), "{listing}");
    // Without an origin there is no webhook URL, so none is invented.
    assert!(!listing.contains("/w/"), "{listing}");
}

#[test]
fn the_listing_reports_addresses_but_never_a_provider_secret() {
    let machine = configured();
    for (name, value) in [
        ("twilio_auth_token", "private-twilio-token"),
        ("resend_api_key", "private-resend-key"),
        ("resend_webhook_signing_secret", "private-signing-secret"),
    ] {
        machine.ok(&["env", "set", "-w", "brain", &format!("{name}={value}")]);
    }

    let listing = machine.ok(&["receiver"]);

    for secret in [
        "private-twilio-token",
        "private-resend-key",
        "private-signing-secret",
    ] {
        assert!(!listing.contains(secret), "leaked {secret} in:\n{listing}");
    }
    assert!(listing.contains("brain@example.test"), "{listing}");
}
