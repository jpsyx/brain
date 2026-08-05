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
    /// Ask the running brain TUI to start receiving SMS and email.
    Start,
    /// Show the receiver server state.
    Status,
    /// Ask the running brain TUI to stop receiving messages.
    Stop,
    /// Restart the TUI-owned receiver server.
    Restart,
    /// Show recent receiver logs.
    Logs,
}

#[derive(Subcommand, Debug)]
pub enum ServerAction {
    /// Show whether the brain server is running and where.
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
    fn receiver_server_commands_parse() {
        let cli = Cli::try_parse_from(["brain", "receiver", "restart"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Receiver(args))
            if matches!(args.action, ReceiverServerAction::Restart)));
    }

    #[test]
    fn receiver_set_allows_interactive_mode() {
        let cli = Cli::try_parse_from(["brain", "receiver", "set"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Receiver(args))
            if matches!(args.action, ReceiverServerAction::Set { assignment: None })));
    }
}
