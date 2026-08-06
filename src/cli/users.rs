//! Portable-user management command grammar.

use clap::{ArgAction, Args, Subcommand};

#[derive(Args, Debug)]
pub struct UserArgs {
    #[command(subcommand)]
    pub action: UserAction,
}

#[derive(Subcommand, Debug)]
pub enum UserAction {
    /// List portable members of the selected workspace.
    List,
    /// Add a portable member.
    Add {
        /// Exact lower-case kebab user ID.
        #[arg(long)]
        id: Option<String>,
        /// Human-facing display name.
        #[arg(long)]
        name: Option<String>,
        /// Enabled inbound phone identity. Repeat to add more than one.
        #[arg(long, action = ArgAction::Append)]
        phone: Vec<String>,
        /// Enabled inbound email identity. Repeat to add more than one.
        #[arg(long, action = ArgAction::Append)]
        email: Vec<String>,
        /// Long-response email for this user.
        #[arg(long)]
        response_email: Option<String>,
    },
    /// Update one portable member.
    Update {
        /// Portable user ID.
        id: Option<String>,
        /// Replacement display name.
        #[arg(long)]
        name: Option<String>,
        /// Enabled inbound phone identity to append. Repeatable.
        #[arg(long, action = ArgAction::Append)]
        add_phone: Vec<String>,
        /// Enabled inbound email identity to append. Repeatable.
        #[arg(long, action = ArgAction::Append)]
        add_email: Vec<String>,
        /// Long-response email for this user.
        #[arg(long)]
        response_email: Option<String>,
    },
    /// Move work from any assignment value onto an existing member.
    Reassign {
        /// Current `assigned_to` value in the task CSVs.
        from: Option<String>,
        /// Existing portable user ID that receives the work.
        to: Option<String>,
    },
    /// Remove one member, optionally reassigning their tasks first.
    Remove {
        /// Portable user ID.
        id: Option<String>,
        /// Existing member that receives assigned tasks.
        #[arg(long)]
        reassign_to: Option<String>,
    },
    /// Select this machine's local person for the workspace.
    Local {
        /// Existing portable user ID.
        id: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;

    #[test]
    fn complete_and_promptable_user_grammars_parse() {
        for args in [
            vec!["brain", "user", "list"],
            vec!["brain", "user", "add"],
            vec!["brain", "user", "add", "--id", "alex", "--name", "Alex"],
            vec!["brain", "user", "update"],
            vec!["brain", "user", "update", "alex", "--name", "Alex R"],
            vec!["brain", "user", "reassign"],
            vec!["brain", "user", "reassign", "me"],
            vec!["brain", "user", "reassign", "me", "alex"],
            vec!["brain", "user", "remove"],
            vec!["brain", "user", "remove", "alex", "--reassign-to", "sam"],
            vec!["brain", "user", "local"],
            vec!["brain", "user", "local", "alex"],
        ] {
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
    }
}
