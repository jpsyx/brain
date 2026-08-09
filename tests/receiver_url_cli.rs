//! `brain receiver url`: the informational webhook-URL surface.
//!
//! Drives the compiled binary with an isolated `HOME`/`XDG_CONFIG_HOME` and
//! never starts a server, which is the point: a provider portal is configured
//! before ingress is ever live.

use std::process::Command;

use tempfile::TempDir;

struct Machine {
    home: TempDir,
    config_home: TempDir,
}

impl Machine {
    /// Two registered workspaces (`brain` is default, `family` is not), each
    /// with a portable local user so ordinary commands are ready.
    fn new() -> Self {
        let machine = Self {
            home: tempfile::tempdir().expect("home tempdir"),
            config_home: tempfile::tempdir().expect("config tempdir"),
        };
        for workspace in ["brain", "family"] {
            let root = machine.home.path().join(workspace);
            machine.ok(&[
                "workspace",
                "create",
                "--name",
                workspace,
                "--root",
                root.to_str().expect("root path"),
            ]);
            machine.ok(&[
                "user", "add", "-w", workspace, "--id", "pablo", "--name", "Pablo",
            ]);
            machine.ok(&["user", "local", "pablo", "-w", workspace]);
        }
        machine
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("run brain {args:?}: {error}"))
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "brain {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("UTF-8 stdout")
    }

    /// The stable ingress UUID from a workspace's portable manifest.
    fn ingress(&self, workspace: &str) -> String {
        let manifest = self
            .home
            .path()
            .join(workspace)
            .join(".config/workspace.json");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).expect("read manifest"))
                .expect("manifest JSON");
        manifest["receiver_ingress_id"]
            .as_str()
            .expect("ingress id")
            .to_owned()
    }
}

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
