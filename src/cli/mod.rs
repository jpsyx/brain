//! Clap surface.
//!
//! Bare `brain` opens the persistent shell in its default tasks view. Focused
//! child modules own each command family while this module preserves the
//! public `crate::cli::*` surface.

mod configuration;
mod contacts;
mod global;
mod internal;
mod project;
mod server;
mod sync;
mod tasks;
mod users;
mod workspace;

pub use configuration::*;
pub use contacts::*;
pub use global::{AgentSelectionError, Cli};
pub use internal::InternalMigrationArgs;
pub use project::*;
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
    /// Run an installer-owned version transition.
    #[command(name = "__migrate", hide = true)]
    InternalMigration(InternalMigrationArgs),

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

    /// Read or change a workspace member's persona (identity + tag styles),
    /// stored per portable user ID in
    /// `<brain-root>/.config/personalization.json`. Bare `brain persona` runs
    /// first-run onboarding when this machine's person has nothing set yet,
    /// otherwise it shows their current values.
    #[command(alias = "personalize")]
    Persona(PersonaArgs),

    /// Manage the bundled brain skills (render + install into the agent registry).
    Skills(SkillsArgs),

    /// Inspect the TUI-lifetime shared HTTP server (`status`, `logs`).
    Server(ServerArgs),

    /// Report every workspace's receiver details, or configure and enable one.
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

    /// Review the backlog, or park, restore, purge, and dedupe it. Bare
    /// `brain backlog` lists what is parked, stalest first.
    Backlog(BacklogArgs),

    /// Remove tool byproducts (Finder metadata, Python caches, editor
    /// scratch) from the workspace root. Idempotent, and `--dry-run` previews.
    Clean(CleanArgs),

    /// The workspace's local contacts book: add, edit, delete, list, and
    /// search. Bare `brain contacts` lists everyone.
    Contacts(ContactsArgs),

    /// Scaffold, edit, archive, and inspect a PARA project. The judgement —
    /// which namespace, which outcome, whether it is really done — stays with
    /// you; the record-keeping does not.
    Project(ProjectArgs),

    /// The deterministic bookkeeping behind triage. Running a triage pass is
    /// judgement work an agent does; this owns only the state that must
    /// survive a session ending mid-run.
    Triage(TriageArgs),

    /// Show what would sync (pending local pushes and remote pulls) without
    /// syncing. Read-only: runs `rclone bisync --dry-run` under the hood.
    Check,

    /// Stop every running Brain shared server and TUI process on this machine.
    Killall,

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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Cmd};

    #[test]
    fn killall_parses_as_a_top_level_command() {
        assert!(matches!(
            Cli::try_parse_from(["brain", "killall"])
                .expect("parse killall")
                .command,
            Some(Cmd::Killall)
        ));
    }
}
