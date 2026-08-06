//! Workspace selection and management command grammar.

use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub action: WorkspaceAction,
}

#[derive(Subcommand, Debug)]
pub enum WorkspaceAction {
    /// List every workspace attached to this machine.
    List,
    /// Create a directory and register it as a new workspace.
    Create {
        /// Canonical workspace name; defaults to the root basename.
        #[arg(long)]
        name: Option<String>,
        /// Root to create and register.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Register an existing root without changing its contents.
    Attach {
        /// Existing root to register.
        root: Option<PathBuf>,
    },
    /// Change a workspace's canonical name while retaining its record.
    Rename {
        /// Existing canonical name or alias.
        workspace: Option<String>,
        /// New canonical name.
        name: Option<String>,
    },
    /// Manage alternative workspace selectors.
    Alias(WorkspaceAliasArgs),
    /// Choose the workspace selected when `--brain` is omitted.
    Default {
        /// Canonical name or alias.
        workspace: Option<String>,
    },
    /// Detach a workspace without deleting its root or contents.
    Remove {
        /// Canonical name or alias.
        workspace: Option<String>,
    },
    /// Repair the selected workspace's required local setup.
    Repair {
        /// Create a missing portable workspace manifest.
        #[arg(long)]
        manifest: bool,
        /// Set this machine's local user ID for the selected workspace.
        #[arg(long, value_name = "USER_ID")]
        local_user_id: Option<String>,
    },
    /// Upgrade a legacy workspace to the multi-workspace data model.
    Migrate {
        /// Confirm every machine that uses the workspace runs a migration-capable Brain version.
        #[arg(long)]
        acknowledge_all_machines_updated: bool,
    },
}

#[derive(Args, Debug)]
pub struct WorkspaceAliasArgs {
    #[command(subcommand)]
    pub action: WorkspaceAliasAction,
}

#[derive(Subcommand, Debug)]
pub enum WorkspaceAliasAction {
    /// Add an alternative selector.
    Add {
        /// Canonical workspace name or current alias.
        workspace: Option<String>,
        /// Alias to add.
        alias: Option<String>,
    },
    /// Remove an alternative selector.
    Remove {
        /// Canonical workspace name or current alias.
        workspace: Option<String>,
        /// Alias to remove.
        alias: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Cmd, WorkspaceAction, try_parse_from};

    #[test]
    fn workspace_selector_is_global_and_retains_its_raw_value() {
        for (args, expected) in [
            (vec!["brain", "-b", "family", "sync"], "family"),
            (vec!["brain", "sync", "--brain", "fam"], "fam"),
            (
                vec!["brain", "config", "get", "access-mode", "-b", "family"],
                "family",
            ),
            (vec!["brain", "--brain", "family"], "family"),
        ] {
            let cli = Cli::try_parse_from(args).expect("global workspace selector should parse");
            assert_eq!(cli.brain.as_deref(), Some(expected));
        }
    }

    #[test]
    fn workspace_management_grammar_accepts_omitted_interactive_values() {
        for args in [
            vec!["brain", "workspace", "list"],
            vec!["brain", "workspace", "create"],
            vec![
                "brain",
                "workspace",
                "create",
                "--name",
                "family",
                "--root",
                "/brains/family",
            ],
            vec!["brain", "workspace", "attach"],
            vec!["brain", "workspace", "attach", "/brains/family"],
            vec!["brain", "workspace", "rename"],
            vec!["brain", "workspace", "rename", "family", "shared"],
            vec!["brain", "workspace", "alias", "add"],
            vec!["brain", "workspace", "alias", "add", "family", "fam"],
            vec!["brain", "workspace", "alias", "remove"],
            vec!["brain", "workspace", "alias", "remove", "family", "fam"],
            vec!["brain", "workspace", "default"],
            vec!["brain", "workspace", "default", "family"],
            vec!["brain", "workspace", "remove"],
            vec!["brain", "workspace", "remove", "family"],
        ] {
            assert!(
                Cli::try_parse_from(&args).is_ok(),
                "workspace grammar should parse {args:?}"
            );
        }
    }

    #[test]
    fn workspace_selector_after_a_delegated_task_positional_is_extracted_verbatim() {
        for (args, expected) in [
            (
                vec![
                    "brain",
                    "tasks",
                    "today",
                    "--brain",
                    "Family_Raw",
                    "--no-tui",
                ],
                "Family_Raw",
            ),
            (
                vec!["brain", "tasks", "today", "-b", "Fam", "--no-tui"],
                "Fam",
            ),
            (
                vec!["brain", "tasks", "today", "--brain=FAMILY", "--no-tui"],
                "FAMILY",
            ),
        ] {
            let cli = try_parse_from(args).expect("global selector after positional should parse");
            assert_eq!(cli.brain.as_deref(), Some(expected));
            let Some(Cmd::Tasks(tasks)) = cli.command else {
                panic!("expected delegated tasks command");
            };
            assert_eq!(tasks.rest, vec!["today", "--no-tui"]);
        }
    }

    #[test]
    fn option_terminator_keeps_selector_looking_tokens_in_delegated_task_values() {
        let cli = try_parse_from(["brain", "tasks", "today", "--", "--brain", "Family_Raw"])
            .expect("selector-looking delegated values should parse");

        assert!(cli.brain.is_none());
        let Some(Cmd::Tasks(tasks)) = cli.command else {
            panic!("expected delegated tasks command");
        };
        assert_eq!(tasks.rest, vec!["today", "--", "--brain", "Family_Raw"]);
    }

    #[test]
    fn duplicate_and_missing_trailing_workspace_selectors_remain_clap_errors() {
        let duplicate =
            try_parse_from(["brain", "tasks", "today", "--brain", "family", "-b", "work"])
                .expect_err("duplicate selectors must fail");
        assert!(
            duplicate
                .to_string()
                .contains("cannot be used multiple times")
        );

        let missing = try_parse_from(["brain", "tasks", "today", "--brain"])
            .expect_err("missing selector value must fail");
        let message = missing.to_string();
        assert!(message.contains("--brain <WORKSPACE>"));
        assert!(message.contains("value"));
    }

    #[test]
    fn workspace_migrate_accepts_human_and_acknowledged_headless_forms() {
        let human = Cli::try_parse_from(["brain", "workspace", "migrate"]).unwrap();
        let headless = Cli::try_parse_from([
            "brain",
            "workspace",
            "migrate",
            "--brain",
            "family",
            "--acknowledge-all-machines-updated",
        ])
        .unwrap();

        let Some(Cmd::Workspace(human)) = human.command else {
            panic!("expected workspace command");
        };
        assert!(matches!(
            human.action,
            WorkspaceAction::Migrate {
                acknowledge_all_machines_updated: false
            }
        ));
        let Some(Cmd::Workspace(headless)) = headless.command else {
            panic!("expected workspace command");
        };
        assert!(matches!(
            headless.action,
            WorkspaceAction::Migrate {
                acknowledge_all_machines_updated: true
            }
        ));
    }
}
