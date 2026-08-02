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
    Setup,
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
}
