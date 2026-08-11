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
pub struct PersonaArgs {
    #[command(subcommand)]
    pub action: Option<PersonaAction>,
}

#[derive(Subcommand, Debug)]
pub enum PersonaAction {
    /// Print one member's persona as a stable, keyed block (the lookup skills
    /// read). Defaults to this machine's local person.
    Show {
        /// Portable user ID. Omit for this machine's local person.
        #[arg(long)]
        user: Option<String>,
    },
    /// Print every workspace member's persona, the local one marked.
    List,
    /// Print everything brain knows about one member, or one field of theirs:
    /// `brain persona get pablo`, `brain persona get pablo role`.
    Get {
        /// Portable user ID.
        user: String,
        /// Field name (`name`, `role`, `works_for`). Omit for the whole persona.
        field: Option<String>,
    },
    /// Set a field: `brain persona set <field>=<value> [--user <id>]`.
    Set {
        /// A single `field=value` assignment.
        assignment: String,
        /// Portable user ID. Omit for this machine's local person.
        #[arg(long)]
        user: Option<String>,
    },
    /// Open the raw personas JSON in `$EDITOR` (for editing `tag_styles`).
    Edit,
}

#[derive(Args, Debug)]
pub struct SkillsArgs {
    #[command(subcommand)]
    pub action: Option<SkillsAction>,
}

#[derive(Subcommand, Debug)]
pub enum SkillsAction {
    /// Render + install bundled skills into the selected workspace and frontends.
    Sync {
        /// Install under this sandbox dir instead of the real per-user layout
        /// (for testing; installs below this workspace instead of the selected one).
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
    fn every_persona_grammar_parses_and_addresses_a_user() {
        use super::PersonaAction;

        let action = |args: &[&str]| {
            let cli = Cli::try_parse_from(args).unwrap_or_else(|error| panic!("{args:?}: {error}"));
            let Some(Cmd::Persona(persona)) = cli.command else {
                panic!("{args:?} is not a persona command");
            };
            persona.action
        };

        assert!(action(&["brain", "persona"]).is_none());
        assert!(matches!(
            action(&["brain", "persona", "show"]),
            Some(PersonaAction::Show { user: None })
        ));
        assert!(matches!(
            action(&["brain", "persona", "show", "--user", "sam"]),
            Some(PersonaAction::Show { user: Some(user) }) if user == "sam"
        ));
        assert!(matches!(
            action(&["brain", "persona", "list"]),
            Some(PersonaAction::List)
        ));
        assert!(matches!(
            action(&["brain", "persona", "get", "pablo"]),
            Some(PersonaAction::Get { user, field: None }) if user == "pablo"
        ));
        assert!(matches!(
            action(&["brain", "persona", "get", "pablo", "role"]),
            Some(PersonaAction::Get { user, field: Some(field) })
                if user == "pablo" && field == "role"
        ));
        assert!(matches!(
            action(&["brain", "persona", "set", "role=CEO"]),
            Some(PersonaAction::Set { assignment, user: None }) if assignment == "role=CEO"
        ));
        assert!(matches!(
            action(&["brain", "persona", "set", "role=CEO", "--user", "sam"]),
            Some(PersonaAction::Set { user: Some(user), .. }) if user == "sam"
        ));
        assert!(matches!(
            action(&["brain", "persona", "edit"]),
            Some(PersonaAction::Edit)
        ));
    }

    #[test]
    fn personalize_stays_a_hidden_alias_for_persona() {
        // Muscle memory (and any script written before personas were per-user)
        // keeps working, but only `persona` is advertised.
        let cli = Cli::try_parse_from(["brain", "personalize", "list"]).expect("legacy alias");
        assert!(matches!(cli.command, Some(Cmd::Persona(_))));

        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("persona"), "{help}");
        assert!(!help.contains("personalize"), "{help}");
    }

    #[test]
    fn skills_status_is_a_noninteractive_reporting_action() {
        let cli = Cli::try_parse_from(["brain", "skills", "status"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Skills(args))
            if matches!(args.action, Some(SkillsAction::Status))));
    }
}
