//! Background and receiver server command grammar.

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct ServerArgs {
    #[command(subcommand)]
    pub action: ServerAction,
}

#[derive(Args, Debug)]
pub struct ReceiverArgs {
    #[command(subcommand)]
    pub action: ReceiverServerAction,
}

#[derive(Subcommand, Debug)]
pub enum ReceiverServerAction {
    /// Interactively configure receiver addresses and allowlists.
    Setup,
    /// Set one receiver environment variable, or choose interactively.
    Set {
        /// `name=value`; omit to choose from the receiver environment variables.
        assignment: Option<String>,
    },
    /// Persistently enable receiver ingress for the selected workspace.
    Start,
    /// Show persistent receiver intent and current availability.
    Status,
    /// Persistently disable receiver ingress for the selected workspace.
    Stop,
    /// Show recent receiver logs.
    Logs,
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
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::ReceiverServerAction;
    use crate::cli::{Cli, Cmd};

    #[test]
    fn receiver_commands_expose_enablement_but_no_manual_lifecycle() {
        for action in ["setup", "set", "start", "stop", "status", "logs"] {
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
        let cli = crate::cli::try_parse_from(["brain", "--with-receiver", "-b", "family"])
            .expect("parse startup receiver selection");

        assert!(cli.with_receiver);
        assert_eq!(cli.brain.as_deref(), Some("family"));
    }

    #[test]
    fn receiver_set_allows_interactive_mode() {
        let cli = Cli::try_parse_from(["brain", "receiver", "set"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Receiver(args))
            if matches!(args.action, ReceiverServerAction::Set { assignment: None })));
    }
}
