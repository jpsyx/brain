//! Sync command grammar.

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub action: Option<SyncAction>,
    /// Bias this run to the local side (local wins same-file conflicts).
    #[arg(long, global = true)]
    pub push: bool,
    /// Bias this run to the remote side (remote wins same-file conflicts).
    #[arg(long, global = true)]
    pub pull: bool,
    /// Internal: run only if no sync is already in progress, otherwise exit
    /// silently (coalesce). Used by the detached background triggers so they
    /// never stack up; a user-run `brain sync` omits it and instead follows an
    /// in-flight sync.
    #[arg(long, global = true, hide = true)]
    pub if_idle: bool,
}

#[derive(Subcommand, Debug)]
pub enum SyncAction {
    /// Configure the B2 bucket credentials and establish the baseline.
    Setup {
        /// Adopt a nonempty manifestless remote only when this exactly matches
        /// the selected workspace UUID.
        #[arg(long, value_name = "WORKSPACE_UUID")]
        adopt_workspace_id: Option<String>,
    },
    /// Repair sync metadata by recreating the marker and baseline.
    Repair,
    /// Deprecated alias for `repair`; kept hidden for old docs/scripts.
    #[command(hide = true)]
    Init,
    /// Show the last run, pending changes, and open conflicts.
    Status,
    /// List open conflict copies. With `--json`, emit structured JSON
    /// (one object per original, with its copies + filesystem metadata)
    /// instead of the themed human-readable list.
    Conflicts {
        /// Emit structured JSON instead of the themed human-readable list.
        #[arg(long)]
        json: bool,
    },
    /// Delete the resolved conflict copies for one or more canonical originals
    /// (after you have merged into them). With no argument, pick interactively.
    Resolve {
        /// Canonical original path(s) to resolve (relative to the brain root).
        originals: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::{SyncAction, SyncArgs};
    use crate::cli::Cli;

    #[test]
    fn sync_help_retains_setup_repair_and_resolve_guidance() {
        let help = Cli::try_parse_from(["brain", "sync", "--help"])
            .unwrap_err()
            .to_string();

        assert!(help.contains("credentials and establish the baseline"));
        assert!(help.contains("recreating the marker and baseline"));
        assert!(help.contains("With no argument, pick interactively"));
        assert!(help.contains("one object per original"));
    }

    #[test]
    fn setup_accepts_only_an_exact_workspace_uuid_as_noninteractive_adoption_authority() {
        let cli = Cli::try_parse_from([
            "brain",
            "sync",
            "setup",
            "--adopt-workspace-id",
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        ])
        .expect("dedicated adoption authority flag");

        let Some(crate::cli::Cmd::Sync(SyncArgs {
            action: Some(SyncAction::Setup { adopt_workspace_id }),
            ..
        })) = cli.command
        else {
            panic!("expected sync setup arguments");
        };
        assert_eq!(
            adopt_workspace_id.as_deref(),
            Some("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        );
        assert!(
            Cli::try_parse_from(["brain", "sync", "setup", "--yes"]).is_err(),
            "a generic confirmation flag must not authorize adoption"
        );

        let help = Cli::try_parse_from(["brain", "sync", "setup", "--help"])
            .unwrap_err()
            .to_string();
        assert!(help.contains("--adopt-workspace-id <WORKSPACE_UUID>"));
        assert!(help.contains("nonempty manifestless remote"));
    }
}
