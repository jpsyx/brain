//! Background and receiver server command grammar.

use clap::{Args, Subcommand, ValueEnum};

#[derive(Args, Debug)]
pub struct ServerArgs {
    #[command(subcommand)]
    pub action: ServerAction,
}

#[derive(Args, Debug)]
pub struct ReceiverArgs {
    /// Omit for every registered workspace's receiver details (`-w` narrows to one).
    #[command(subcommand)]
    pub action: Option<ReceiverServerAction>,
}

#[derive(Subcommand, Debug)]
pub enum ReceiverServerAction {
    /// Configure selected-workspace providers and portable user mappings.
    Setup(Box<ReceiverSetupArgs>),
    /// Set one receiver environment variable, or choose interactively.
    Set {
        /// `name=value`; omit to choose from the receiver environment variables.
        assignment: Option<String>,
    },
    /// Persistently enable receiver ingress for the selected workspace.
    Start,
    /// Show persistent receiver intent and current availability.
    Status,
    /// Print the email address the selected workspace's receiver answers on.
    ///
    /// The bare value on stdout, so a script or an agent can read it without
    /// parsing a status block. `-w` asks about another workspace.
    Email,
    /// Print the phone number the selected workspace's receiver answers on.
    ///
    /// The bare value on stdout, so a script or an agent can read it without
    /// parsing a status block. `-w` asks about another workspace.
    Phone,
    /// Print the exact webhook URLs to paste into the provider portals.
    ///
    /// Informational only: it reads this machine's public base URL and the
    /// workspace's portable ingress UUID, so it works before receiver ingress
    /// is ever enabled or running. Combine with `-w` for another workspace.
    Url(ReceiverUrlArgs),
    /// Persistently disable receiver ingress for the selected workspace.
    Stop,
    /// Show recent receiver logs.
    Logs,
}

#[derive(Args, Debug, Default)]
pub struct ReceiverUrlArgs {
    /// Print only the Twilio SMS webhook URL.
    #[arg(long)]
    pub sms: bool,
    /// Print only the Resend email webhook URL.
    #[arg(long)]
    pub email: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReceiverSetupChannels {
    Email,
    Sms,
    Both,
}

#[derive(Args, Debug, Default)]
pub struct ReceiverSetupArgs {
    /// Receiver channels to configure. Omit for the guided setup.
    #[arg(long, value_enum)]
    pub channels: Option<ReceiverSetupChannels>,
    /// Public HTTPS base URL that fronts this Brain machine.
    #[arg(long)]
    pub public_url: Option<String>,
    /// Twilio account identifier for the selected workspace.
    #[arg(long)]
    pub twilio_account_sid: Option<String>,
    /// Twilio request-signing secret for the selected workspace.
    #[arg(long)]
    pub twilio_auth_token: Option<String>,
    /// Twilio sender phone number, including country code.
    #[arg(long)]
    pub twilio_from_number: Option<String>,
    /// Resend credential used only to send replies (scope: sending access only).
    #[arg(long)]
    pub resend_sending_api_key: Option<String>,
    /// Resend credential used only to read inbound email (must be full access).
    #[arg(long)]
    pub resend_full_access_api_key: Option<String>,
    /// Verified Resend sender address for outbound delivery.
    #[arg(long)]
    pub resend_from_email: Option<String>,
    /// Resend webhook-signing secret for inbound verification.
    #[arg(long)]
    pub resend_webhook_signing_secret: Option<String>,
    /// Existing portable user ID, or the ID for a new user.
    #[arg(long)]
    pub user_id: Option<String>,
    /// Display name required when `--user-id` creates a new user.
    #[arg(long)]
    pub user_name: Option<String>,
    /// Phone identity to map when SMS is configured.
    #[arg(long)]
    pub phone: Option<String>,
    /// Whether the supplied phone may initiate inbound work.
    #[arg(long, requires = "phone")]
    pub phone_allowed: Option<bool>,
    /// Email identity to map when email is configured.
    #[arg(long)]
    pub email: Option<String>,
    /// Whether the supplied email may initiate inbound work.
    #[arg(long, requires = "email")]
    pub email_allowed: Option<bool>,
    /// Optional long-response address for this user.
    #[arg(long)]
    pub response_email: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum ServerAction {
    /// Show whether the shared process is running and its live TUI count.
    Status,
    /// Show recent shared-server lifecycle logs.
    Logs,
    /// (internal) Run the elected blocking server loop.
    #[command(hide = true)]
    Run {
        #[arg(long)]
        generation: crate::server::lifecycle::ServerGeneration,
        #[arg(long)]
        port: u16,
        #[arg(long, hide = true)]
        background: bool,
    },
}

#[cfg(test)]
mod tests {
    use clap::{Args as _, Parser};

    use super::ReceiverServerAction;
    use crate::cli::{Cli, Cmd};

    #[test]
    fn receiver_setup_help_explains_every_provider_flag() {
        let help = super::ReceiverSetupArgs::augment_args(clap::Command::new("setup"))
            .render_long_help()
            .to_string();
        for description in [
            "Twilio account identifier",
            "Twilio request-signing secret",
            "Twilio sender phone number",
            "Resend credential used only to send replies",
            "Resend credential used only to read inbound email",
            "Verified Resend sender address",
            "Resend webhook-signing secret",
        ] {
            assert!(
                help.contains(description),
                "missing `{description}` in:\n{help}"
            );
        }
    }

    #[test]
    fn receiver_url_parses_bare_and_with_either_channel_flag() {
        for arguments in [
            vec!["brain", "receiver", "url"],
            vec!["brain", "receiver", "url", "--sms"],
            vec!["brain", "receiver", "url", "--email"],
            // Naming both channels is redundant, not a conflict.
            vec!["brain", "receiver", "url", "--sms", "--email"],
            // The global workspace selector composes on either side.
            vec!["brain", "receiver", "url", "-w", "family"],
            vec!["brain", "-w", "family", "receiver", "url", "--sms"],
        ] {
            let cli = Cli::try_parse_from(&arguments)
                .unwrap_or_else(|error| panic!("{arguments:?}: {error}"));
            assert!(
                matches!(cli.command, Some(Cmd::Receiver(args))
                    if matches!(args.action, Some(ReceiverServerAction::Url(_)))),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn receiver_url_is_classified_as_a_read_only_status_invocation() {
        // Informational only: it must not take the ready-workspace path, which
        // would make a purely-printing command depend on workspace mutations.
        let cli = Cli::try_parse_from(["brain", "receiver", "url"]).expect("parse");

        assert!(crate::workspace::is_read_only_status(&cli));
    }

    #[test]
    fn bare_receiver_parses_as_the_details_listing() {
        let cli = Cli::try_parse_from(["brain", "receiver"]).expect("parse");

        assert!(matches!(cli.command, Some(Cmd::Receiver(args)) if args.action.is_none()));
    }

    #[test]
    fn receiver_email_and_phone_parse_bare_and_with_the_workspace_selector() {
        for (arguments, expected) in [
            (vec!["brain", "receiver", "email"], "email"),
            (vec!["brain", "receiver", "phone"], "phone"),
            (vec!["brain", "receiver", "email", "-w", "family"], "email"),
            (vec!["brain", "-w", "family", "receiver", "phone"], "phone"),
        ] {
            let cli = Cli::try_parse_from(&arguments)
                .unwrap_or_else(|error| panic!("{arguments:?}: {error}"));
            let Some(Cmd::Receiver(args)) = cli.command else {
                panic!("{arguments:?} should route to receiver");
            };
            let matched = match expected {
                "email" => matches!(args.action, Some(ReceiverServerAction::Email)),
                _ => matches!(args.action, Some(ReceiverServerAction::Phone)),
            };
            assert!(matched, "{arguments:?}");
        }
    }

    #[test]
    fn the_details_listing_and_both_addresses_are_read_only_status_invocations() {
        // Printing configuration must never take the ready-workspace path: these
        // are exactly the commands a half-configured workspace needs to answer.
        for arguments in [
            vec!["brain", "receiver"],
            vec!["brain", "receiver", "email"],
            vec!["brain", "receiver", "phone"],
        ] {
            let cli = Cli::try_parse_from(&arguments).expect("parse");

            assert!(crate::workspace::is_read_only_status(&cli), "{arguments:?}");
        }
    }

    #[test]
    fn receiver_commands_expose_enablement_but_no_manual_lifecycle() {
        for action in [
            "setup", "set", "start", "stop", "status", "logs", "url", "email", "phone",
        ] {
            assert!(
                Cli::try_parse_from(["brain", "receiver", action]).is_ok(),
                "receiver {action} should parse"
            );
        }
        for args in [
            vec!["brain", "receiver", "restart"],
            vec!["brain", "server", "start"],
            vec!["brain", "server", "kill"],
            vec!["brain", "server", "restart"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn with_receiver_and_workspace_selector_parse_together() {
        let cli = crate::cli::try_parse_from(["brain", "--with-receiver", "-w", "family"])
            .expect("parse startup receiver selection");

        assert!(cli.with_receiver);
        assert_eq!(cli.workspace_selector.as_deref(), Some("family"));
    }

    #[test]
    fn receiver_set_allows_interactive_mode() {
        let cli = Cli::try_parse_from(["brain", "receiver", "set"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Receiver(args))
            if matches!(args.action, Some(ReceiverServerAction::Set { assignment: None }))));
    }

    #[test]
    fn receiver_setup_exposes_complete_headless_user_mapping() {
        let cli = Cli::try_parse_from([
            "brain",
            "receiver",
            "setup",
            "--channels",
            "sms",
            "--public-url",
            "https://brain.example.test",
            "--twilio-account-sid",
            "AC123",
            "--twilio-auth-token",
            "secret",
            "--twilio-from-number",
            "+12125550100",
            "--user-id",
            "alex",
            "--user-name",
            "Alex",
            "--phone",
            "+12125550101",
            "--phone-allowed",
            "false",
        ])
        .expect("parse headless setup");

        assert!(matches!(cli.command, Some(Cmd::Receiver(args))
            if matches!(&args.action, Some(ReceiverServerAction::Setup(setup))
                if setup.phone_allowed == Some(false))));
    }
}
