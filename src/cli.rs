//! Clap surface.
//!
//! Subcommands: `tasks` (open the tasks view, or run a tasks utility) and
//! `config` (read/change persistent config). Bare `brain` opens the persistent
//! shell in its default (tasks) view.

use clap::{Args, Parser, Subcommand};

#[must_use]
pub fn parse() -> Cli {
    Cli::parse_from(normalize_codex_aliases(std::env::args()))
}

fn normalize_codex_aliases<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    args.into_iter()
        .map(Into::into)
        .map(|arg| {
            if arg == "-cx" {
                "--codex".to_owned()
            } else {
                arg
            }
        })
        .collect()
}

#[derive(Parser, Debug)]
#[command(
    name = "brain",
    version = env!("CARGO_PKG_VERSION"),
    disable_version_flag = true,
    about = "Brain CLI: central terminal dispatch for ~/brain and the task system.",
    long_about = "Brain CLI: the central terminal dispatch for the user's second\n\
                  brain and task system. Bare `brain` opens a persistent shell\n\
                  with two main views (tasks: management, agenda, triage, the\n\
                  startup default; and a fuzzy search over ~/brain), plus an\n\
                  app-level brain panel running an interactive agent session.\n\
                  \n\
                  Subcommands:\n\
                  \n\
                    tasks     Open the tasks view, or run a tasks utility\n\
                              (`brain tasks today --no-tui`,\n\
                              `brain tasks complete t123`,\n\
                              `brain tasks doctor`,\n\
                              `brain tasks search lamaze`).\n\
                  \n\
                    config    Read or change persistent config\n\
                              (`brain config`, `brain config get <name>`,\n\
                              `brain config set <name>=<value>`).\n\
                  \n\
                  Inside the shell: Ctrl-L/Ctrl-H cycle views, Ctrl-T/Ctrl-B\n\
                  jump to the tasks / brain-search view, Ctrl-P opens the\n\
                  command palette, and Alt-? shows help."
)]
pub struct Cli {
    /// Print the brain version.
    #[arg(short = 'v', long = "version", action = clap::ArgAction::SetTrue)]
    pub print_version: bool,

    /// Mirror the run log to stdout (the log file is always collected).
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Use Codex instead of Claude for the brain panel. Alias: -cx.
    #[arg(long, global = true)]
    pub codex: bool,

    /// Start the TUI-owned receiver server alongside the brain shell.
    #[arg(long, global = true)]
    pub with_receiver: bool,

    /// Never open the daily-triage startup nudge for this run. Process-scoped
    /// (not a persistent config change): brain won't tell you triage hasn't run.
    #[arg(long, global = true)]
    pub no_daily_triage_check: bool,

    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[must_use]
pub fn version_line() -> String {
    format!("brain {}\n", env!("CARGO_PKG_VERSION"))
}

impl Cli {
    /// Selected brain-panel agent frontend.
    #[must_use]
    pub const fn agent_kind(&self) -> crate::session::AgentKind {
        if self.codex {
            crate::session::AgentKind::Codex
        } else {
            crate::session::AgentKind::Claude
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::AgentKind;

    #[test]
    fn codex_flag_selects_codex_frontend() {
        let cli = Cli::try_parse_from(["brain", "--codex"]).expect("parse");
        assert!(cli.codex);
        assert_eq!(cli.agent_kind(), AgentKind::Codex);
    }

    #[test]
    fn cx_alias_selects_codex_frontend() {
        let cli = Cli::try_parse_from(normalize_codex_aliases(["brain", "-cx"])).expect("parse");
        assert!(cli.codex);
        assert_eq!(cli.agent_kind(), AgentKind::Codex);
    }

    #[test]
    fn claude_is_the_default_frontend() {
        let cli = Cli::try_parse_from(["brain"]).expect("parse");
        assert_eq!(cli.agent_kind(), AgentKind::Claude);
    }

    #[test]
    fn with_receiver_is_opt_in() {
        assert!(!Cli::try_parse_from(["brain"]).expect("parse").with_receiver);
        assert!(
            Cli::try_parse_from(["brain", "--with-receiver"])
                .expect("parse")
                .with_receiver
        );
    }

    #[test]
    fn no_daily_triage_check_is_opt_in() {
        assert!(
            !Cli::try_parse_from(["brain"])
                .expect("parse")
                .no_daily_triage_check
        );
        assert!(
            Cli::try_parse_from(["brain", "--no-daily-triage-check"])
                .expect("parse")
                .no_daily_triage_check
        );
    }

    #[test]
    fn receiver_server_commands_parse() {
        let cli = Cli::try_parse_from(["brain", "receiver", "restart"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Receiver(args))
            if matches!(args.action, ReceiverServerAction::Restart)));
    }

    #[test]
    fn reindex_flags_parse_and_default_to_all() {
        let bare = Cli::try_parse_from(["brain", "reindex"]).expect("parse");
        assert!(matches!(bare.command, Some(Cmd::Reindex(args))
            if !args.projects && !args.resources && !args.tasks));
        let scoped = Cli::try_parse_from(["brain", "reindex", "--resources"]).expect("parse");
        assert!(matches!(scoped.command, Some(Cmd::Reindex(args))
            if args.resources && !args.projects && !args.tasks));
    }

    #[test]
    fn receiver_and_env_set_allow_interactive_mode() {
        let receiver = Cli::try_parse_from(["brain", "receiver", "set"]).expect("parse");
        assert!(matches!(receiver.command, Some(Cmd::Receiver(args))
            if matches!(args.action, ReceiverServerAction::Set { assignment: None })));
        let env = Cli::try_parse_from(["brain", "env", "set"]).expect("parse");
        assert!(matches!(env.command, Some(Cmd::Env(args))
            if matches!(args.action, Some(EnvAction::Set { assignment: None }))));
    }

    #[test]
    fn bare_habits_has_no_action() {
        let cli = Cli::try_parse_from(["brain", "habits"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Habits(args)) if args.action.is_none()));
    }

    #[test]
    fn habits_revive_joins_trailing_words() {
        let cli =
            Cli::try_parse_from(["brain", "habits", "revive", "send", "team", "status", "update"])
                .expect("parse");
        let Some(Cmd::Habits(args)) = cli.command else {
            panic!("expected habits");
        };
        let Some(HabitsAction::Revive(revive)) = args.action else {
            panic!("expected revive");
        };
        assert_eq!(revive.query, vec!["send", "team", "status", "update"]);
    }

    #[test]
    fn habits_fix_is_an_alias_for_revive() {
        let cli = Cli::try_parse_from(["brain", "habits", "fix", "meds"]).expect("parse");
        let Some(Cmd::Habits(args)) = cli.command else {
            panic!("expected habits");
        };
        let Some(HabitsAction::Revive(revive)) = args.action else {
            panic!("expected revive");
        };
        assert_eq!(revive.query, vec!["meds"]);
    }

    #[test]
    fn habits_revive_requires_a_query() {
        assert!(Cli::try_parse_from(["brain", "habits", "revive"]).is_err());
    }
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Open the merged shell in the tasks view, or run a tasks utility.
    ///
    /// All arguments after `tasks` are delegated verbatim to the tasks CLI
    /// parser, so `brain tasks`, `brain tasks today --no-tui`,
    /// `brain tasks complete t123`, `brain tasks doctor`, and
    /// `brain tasks search lamaze` all work. Bare `brain` is equivalent to
    /// `brain tasks` (the tasks view is the startup default).
    Tasks(TasksArgs),

    /// Print the brain version.
    Version,

    /// Read or change brain's portable config (`<brain-root>/.config/config.json`,
    /// synced with the brain). Machine-local settings live in `brain env` instead.
    Config(ConfigArgs),

    /// Read or change your machine-local brain env (`~/.config/brain/env.json`):
    /// `root`, `markdown_to_pdf_path`, agent commands, and the Backblaze `sync` block.
    Env(EnvArgs),

    /// Sync your brain across machines via Backblaze B2 (`brain sync setup` first).
    Sync(SyncArgs),

    /// Read or change your personalization (identity + tag styles), stored at
    /// `<brain-root>/.config/personalization.json`. Bare `brain personalize` runs
    /// first-run onboarding if nothing is set yet, otherwise it shows your
    /// current values.
    Personalize(PersonalizeArgs),

    /// Manage the bundled brain skills (render + install into the agent registry).
    Skills(SkillsArgs),

    /// Manage the background brain server (the local HTTP service; `start`,
    /// `status`, `kill`). One shared daemon per machine.
    Server(ServerArgs),

    /// Control the TUI-owned external receiver server.
    #[command(name = "receiver")]
    Receiver(ReceiverArgs),

    /// Open today's habits page, or repair a lapsed habit (`brain habits revive <name>`).
    ///
    /// Bare `brain habits` opens the browser page (starting the brain server if
    /// needed). `brain habits revive <fuzzy name>` (alias `fix`) respawns a
    /// recurring habit whose chain lapsed — every occurrence marked done with
    /// none pending.
    Habits(HabitsArgs),

    /// Show what would sync (pending local pushes and remote pulls) without
    /// syncing. Read-only: runs `rclone bisync --dry-run` under the hood.
    Check,

    /// Rebuild the derived lookup CSVs (`projects-lookup.csv`,
    /// `zotero-lookup.csv`) from the canonical `.METADATA.json` + `notes.md`,
    /// and re-apply the task/habit automation rules. Bare `brain reindex` does
    /// all three; narrow with `--projects` / `--resources` / `--tasks`.
    Reindex(ReindexArgs),
}

#[derive(Args, Debug)]
pub struct HabitsArgs {
    #[command(subcommand)]
    pub action: Option<HabitsAction>,
}

#[derive(Subcommand, Debug)]
pub enum HabitsAction {
    /// Respawn a lapsed habit by fuzzy name (alias: `fix`). A lapsed habit is
    /// one whose every occurrence is `done` with none pending, so it silently
    /// dropped off the agenda. Multiple words are joined, so quotes are
    /// optional: `brain habits revive send team status update`.
    #[command(alias = "fix")]
    Revive(ReviveArgs),
}

#[derive(Args, Debug)]
pub struct ReviveArgs {
    /// The fuzzy habit name to match (all trailing words are joined with spaces).
    #[arg(required = true, num_args = 1..)]
    pub query: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ReindexArgs {
    /// Rebuild only `projects-lookup.csv`.
    #[arg(long)]
    pub projects: bool,
    /// Rebuild only `zotero-lookup.csv`.
    #[arg(long)]
    pub resources: bool,
    /// Re-apply only the task/habit automation rules.
    #[arg(long)]
    pub tasks: bool,
}

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
    /// dot notation, for example `sync.b2_bucket`.
    Set {
        /// A single `name=value` assignment.
        assignment: Option<String>,
    },
}

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
    /// never stack up; a user-run `brain sync` omits it and instead *follows* an
    /// in-flight sync.
    #[arg(long, global = true, hide = true)]
    pub if_idle: bool,
}

#[derive(Subcommand, Debug)]
pub enum SyncAction {
    /// Configure the B2 bucket + credentials and establish the baseline.
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
    /// (after you've merged into them). With no argument, pick interactively.
    Resolve {
        /// Canonical original path(s) to resolve (relative to the brain root).
        originals: Vec<String>,
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
}

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
    /// Set one receiver environment variable, or choose one interactively.
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
    /// Start the brain server in the background (reuses a running one).
    Start,
    /// Show whether the brain server is running and where.
    Status,
    /// Stop the background brain server.
    Kill,
    /// (internal) Run the blocking server loop; used by the background daemon.
    #[command(hide = true)]
    Run {
        #[arg(long)]
        port: u16,
    },
}

#[derive(Args, Debug)]
pub struct TasksArgs {
    /// Everything after `tasks`, handed to the tasks CLI parser unchanged
    /// (positional view/date/search tokens, filter flags, and the
    /// `complete` / `doctor` / `search` subcommands).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}
