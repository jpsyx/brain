//! Clap surface.
//!
//! Bare `brain` opens the persistent shell in its default tasks view. Focused
//! child modules own each command family while this module preserves the
//! public `crate::cli::*` surface.

mod configuration;
mod global;
mod server;
mod sync;
mod tasks;
mod users;
mod workspace;

pub use configuration::*;
pub use global::{AgentSelectionError, Cli};
pub use server::*;
pub use sync::*;
pub use tasks::*;
pub use users::*;
pub use workspace::*;

use clap::{Parser, Subcommand};

#[must_use]
pub fn parse() -> Cli {
    try_parse_from(std::env::args()).unwrap_or_else(|error| error.exit())
}

/// Parse an injected argument stream through the same global normalization as
/// the real process entry point.
pub fn try_parse_from<I, S>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Cli::try_parse_from(global::normalize_global_args(args))
}

#[must_use]
pub fn version_line() -> String {
    format!("brain {}\n", env!("CARGO_PKG_VERSION"))
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
    /// machine values plus the registry-owned, read-only `root` for the selected
    /// workspace.
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

    /// Inspect the TUI-lifetime shared HTTP server (`status`, `logs`).
    Server(ServerArgs),

    /// Configure and enable receiver ingress for the selected workspace.
    #[command(name = "receiver")]
    Receiver(ReceiverArgs),

    /// Open today's habits page, or manage a habit
    /// (`brain habits revive <name>`, `brain habits skip <id>`).
    ///
    /// Bare `brain habits` opens the browser page and starts a background
    /// server when no brain TUI is open. `brain habits kill` stops that
    /// background server when no brain TUI is open. `brain habits revive
    /// <fuzzy name>` (alias `fix`) respawns a
    /// recurring habit whose chain lapsed, meaning every occurrence is marked
    /// done with none pending. `brain habits skip <id|fuzzy>` opts out of a
    /// habit for today (cadence-aware: a daily habit is marked done + respawned;
    /// a non-daily habit is deferred one day; `--until` defers to a given date).
    Habits(HabitsArgs),

    /// Show what would sync (pending local pushes and remote pulls) without
    /// syncing. Read-only: runs `rclone bisync --dry-run` under the hood.
    Check,

    /// Rebuild the derived lookup CSVs (`projects-lookup.csv`,
    /// `zotero-lookup.csv`) from the canonical `.METADATA.json` + `notes.md`,
    /// and re-apply the task/habit automation rules. Bare `brain reindex` does
    /// all three; narrow with `--projects` / `--resources` / `--tasks`.
    Reindex(ReindexArgs),

    /// Select, attach, and manage machine-local workspace registrations.
    Workspace(WorkspaceArgs),

    /// Manage portable members of the selected workspace.
    User(UserArgs),
}
