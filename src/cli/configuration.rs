//! Config, environment, personalization, and skill command grammar.

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: Option<ConfigAction>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print every config variable, its value, and its description as a table.
    List,
    /// Print the effective value of one variable.
    Get {
        /// Variable name (e.g. `linear_workspace`).
        name: String,
    },
    /// Set a variable: `brain config set <name>=<value>`.
    Set {
        /// A single `name=value` assignment. Omit to choose interactively.
        assignment: String,
    },
}

#[derive(Args, Debug)]
pub struct EnvArgs {
    #[command(subcommand)]
    pub action: Option<EnvAction>,
}

#[derive(Subcommand, Debug)]
pub enum EnvAction {
    /// Print every env variable, its value, and its description as a table.
    List,
    /// Print the effective value of one env variable.
    Get {
        /// Variable name (e.g. `root`).
        name: String,
    },
    /// Set an env variable: `brain env set <name>=<value>`. Nested values use
    /// dot notation, for example `sync.b2_bucket`. Structural workspace fields
    /// are read-only and are managed with `brain workspace`.
    Set {
        /// A single `name=value` assignment.
        assignment: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct PersonalizeArgs {
    #[command(subcommand)]
    pub action: Option<PersonalizeAction>,
}

#[derive(Subcommand, Debug)]
pub enum PersonalizeAction {
    /// Print your personalization as a stable, keyed block (the lookup skills read).
    Show,
    /// Print one field's value (`name`, `role`, `works_for`).
    Get {
        /// Field name.
        field: String,
    },
    /// Set a field: `brain personalize set <field>=<value>`.
    Set {
        /// A single `field=value` assignment.
        assignment: String,
    },
    /// Open the raw personalization JSON in `$EDITOR` (for editing `tag_styles`).
    Edit,
}

#[derive(Args, Debug)]
pub struct SkillsArgs {
    #[command(subcommand)]
    pub action: Option<SkillsAction>,
}

#[derive(Subcommand, Debug)]
pub enum SkillsAction {
    /// Render + install the bundled skills into the agent registry and frontends.
    Sync {
        /// Install under this sandbox dir instead of the real per-user layout
        /// (for testing; never touches `~/.agents` or the frontend skill dirs).
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// Show requested workspace capabilities, availability, and enforcement.
    Status,
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{EnvAction, SkillsAction};
    use crate::cli::{Cli, Cmd};

    #[test]
    fn env_set_allows_interactive_mode() {
        let cli = Cli::try_parse_from(["brain", "env", "set"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Env(args))
            if matches!(args.action, Some(EnvAction::Set { assignment: None }))));
    }

    #[test]
    fn env_set_help_says_structural_workspace_fields_are_read_only() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("env")
            .expect("env command")
            .find_subcommand_mut("set")
            .expect("env set command")
            .render_long_help()
            .to_string();

        assert!(
            help.contains("Structural workspace fields are read-only"),
            "{help}"
        );
    }

    #[test]
    fn skills_status_is_a_noninteractive_reporting_action() {
        let cli = Cli::try_parse_from(["brain", "skills", "status"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Skills(args))
            if matches!(args.action, Some(SkillsAction::Status))));
    }
}
