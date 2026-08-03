//! Command-line surface for the tasks view.
//!
//! The flat clap struct is split into three logical groups via
//! `#[command(flatten)]` so callers can pass `&cli.filters` / `&cli.display`
//! without dragging the rest of the struct around.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "tasks",
    version,
    about = "Browse the selected workspace's tasks in a beautiful, scrollable shell.",
    long_about = "Browse the selected workspace's tasks in a beautiful, scrollable shell.\n\
                  \n\
                  By default, shows today's agenda: tasks due today plus everything past-due.\n\
                  Pass a view token to start in a specific view: 'today', 'mit', 'past_due',\n\
                  'week', 'habits', or 'all'. 'habits' lists today's habits from the\n\
                  selected workspace (filtered by recurrence interval). In the shell,\n\
                  press Tab to cycle forward through these modes and Shift+Tab to cycle\n\
                  backward (today → mit → past_due → week → habits → all → today).\n\
                  You can also pass a date (YYYY-MM-DD), 'tomorrow', 'yesterday', or a weekday\n\
                  name like 'friday' / 'next monday' to filter by a specific due date.\n\
                  \n\
                  Subcommands: `tasks complete t123` (alias: `finish`) hands the ID to claude\n\
                  with a `/todo done T123` prompt prefilled — claude does the actual mutation.\n\
                  Natural-language equivalents: `tasks mark t123`, `tasks mark t123 done`,\n\
                  `tasks mark t123 as done` all map to the same complete command.\n\
                  \n\
                  Shell keys: j / ↓ line-down · k / ↑ line-up · d half-page-down ·\n\
                  u half-page-up · PgDn page-down · PgUp page-up ·\n\
                  g top · G bottom · Tab / Shift+Tab cycle view · / live fuzzy-search\n\
                  (matches id + name) · q / Esc / Ctrl-C quit (Esc also clears an active filter)."
)]
pub struct Cli {
    /// Positional input. Empty → 'today' view. A single token that names
    /// a Tab-cycle view ('today', 'mit', 'past_due', 'week', 'habits',
    /// 'backlog', 'all') becomes
    /// the starting view. A token that parses as a one-off selector
    /// ('tomorrow', 'yesterday', a weekday like 'friday' / 'next monday',
    /// or YYYY-MM-DD) opens a custom view (no active view; Tab still cycles).
    /// Anything else (multi-word or an unrecognized single token) is treated
    /// as a free-text search across all tasks.
    pub query: Vec<String>,

    #[command(flatten)]
    pub filters: Filters,

    #[command(flatten)]
    pub display: DisplayOpts,

    /// Path to the tasks CSV (default: the selected workspace's tasks/tasks.csv,
    /// or the value of $BRAIN_TASKS_CSV).
    #[arg(long, env = "BRAIN_TASKS_CSV", value_name = "PATH")]
    pub csv: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Mark a task (or habit) as complete in the native CSV store.
    /// Accepts t123, T123, 123, h43, H43. Aliases: `finish`, `done`.
    #[command(aliases = ["finish", "done"])]
    Complete(CompleteArgs),

    /// Search task names, notes, projects, and IDs for the given terms.
    /// Equivalent to typing the same terms positionally — e.g.
    /// `tasks search lamaze classes` ≡ `tasks lamaze classes`.
    Search(SearchArgs),

    /// Validate that the state DB + Stop hook are wired up. Prints a
    /// one-line-per-check report and exits 0 on full health, 1 if any
    /// check fails.
    Doctor,
}

#[derive(Args, Debug)]
pub struct CompleteArgs {
    /// Task or habit ID: t123, T123, 123 (assumed task), h43, H43.
    pub id: String,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// What to search for. Words are joined with spaces.
    #[arg(required = true, num_args = 1..)]
    pub query: Vec<String>,
}

/// Filters applied on top of the date selector. Default = no filters.
#[derive(Args, Debug, Default)]
pub struct Filters {
    /// Filter by hard_deadline (true|false).
    #[arg(long, value_name = "BOOL", global = true)]
    pub hard_deadline: Option<bool>,

    /// Filter by status (not_started, in_progress, done).
    #[arg(long, value_name = "STATUS", global = true)]
    pub status: Option<String>,

    /// Filter by priority (p0..p4).
    #[arg(long, value_name = "PRIO", global = true)]
    pub priority: Option<String>,

    /// Filter by task type (matches if the task's type set contains this value).
    /// Examples: ceo, aa, personal, code, languages, finance, mit, needs_attention.
    #[arg(long = "type", value_name = "TYPE", global = true)]
    pub task_type: Option<String>,

    /// Filter by project slug.
    #[arg(long, value_name = "SLUG", global = true)]
    pub project: Option<String>,

    /// Filter by energy level (high|medium|low).
    #[arg(long, value_name = "LEVEL", global = true)]
    pub energy: Option<String>,

    /// Filter by GTD context (home|office|computer|calls|errand).
    #[arg(long, value_name = "CTX", global = true)]
    pub context: Option<String>,

    /// Only past-due tasks (due_date < today, status != done).
    #[arg(long, global = true)]
    pub past_due: bool,

    /// Only MIT (Most Important Task) entries.
    #[arg(long, global = true)]
    pub mit: bool,

    /// Only stale tasks (>= 21 days since last_touched, status != done).
    #[arg(long, global = true)]
    pub stale: bool,

    /// Only tasks with no due date set.
    #[arg(long, global = true)]
    pub no_due: bool,

    /// Only tasks that are blocked by another.
    #[arg(long, global = true)]
    pub blocked: bool,

    /// Include done tasks (hidden by default).
    #[arg(long, global = true)]
    pub include_done: bool,

    /// Include deferred tasks whose start_date is still in the future.
    /// By default they are hidden.
    #[arg(long, global = true)]
    pub include_deferred: bool,

    /// Free-text search across task name and notes (case-insensitive).
    #[arg(short = 's', long, value_name = "QUERY", global = true)]
    pub search: Option<String>,
}

/// Output / sorting / formatting toggles.
#[derive(Args, Debug, Default)]
pub struct DisplayOpts {
    /// Sort order: priority (default), due, created, touched, defer.
    #[arg(long, default_value = "priority", value_name = "FIELD", global = true)]
    pub sort: String,

    /// Sort descending.
    #[arg(long, global = true)]
    pub reverse: bool,

    /// Print plain output to stdout instead of launching the tasks shell.
    #[arg(long, global = true)]
    pub no_tui: bool,

    /// Show the long notes field in full (default: truncate to ~120 chars).
    #[arg(long, global = true)]
    pub full_notes: bool,
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn help_describes_tasks_under_the_selected_workspace() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("selected workspace"), "{help}");
        assert!(!help.contains("~/brain/tasks"), "{help}");
    }
}
