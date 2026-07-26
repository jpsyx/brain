//! Clap surface.
//!
//! Subcommands: `tasks` (open the tasks view, or run a tasks utility) and
//! `config` (read/change persistent config). Bare `brain` opens the persistent
//! shell in its default (tasks) view.

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "brain",
    version,
    about = "Brain CLI: central terminal dispatch for ~/brain and the task system.",
    long_about = "Brain CLI: the central terminal dispatch for the user's second\n\
                  brain and task system. Bare `brain` opens a persistent shell\n\
                  with two main views (tasks: management, agenda, triage, the\n\
                  startup default; and a fuzzy search over ~/brain), plus an\n\
                  app-level brain panel running an interactive claude session.\n\
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
    #[command(subcommand)]
    pub command: Option<Cmd>,
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

    /// Read or change brain's portable config (`<brain-root>/.config/config.json`,
    /// synced with the brain). Machine-local settings live in `brain env` instead.
    Config(ConfigArgs),

    /// Read or change your machine-local brain env (`~/.config/brain/env.json`):
    /// `root`, `markdown_to_pdf_path`, and the Backblaze `sync` block.
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

    /// Open today's habits page in your browser (starts the brain server if needed).
    Habits,

    /// Show what would sync (pending local pushes and remote pulls) without
    /// syncing. Read-only: runs `rclone bisync --dry-run` under the hood.
    Check,
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
        /// A single `name=value` assignment.
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
    /// Set an env variable: `brain env set <name>=<value>`.
    Set {
        /// A single `name=value` assignment.
        assignment: String,
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
}

#[derive(Subcommand, Debug)]
pub enum SyncAction {
    /// Configure the B2 bucket + credentials and establish the baseline.
    Setup,
    /// (Re)establish the bisync baseline (first run / recovery / fresh machine).
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
