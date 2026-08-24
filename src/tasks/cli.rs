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

    /// Add a task or habit directly to the selected workspace's native CSV store.
    Add(Box<AddArgs>),

    /// Edit fields on one existing task or habit (absolute values, no defer
    /// penalty). Aliases: `edit`, `update`.
    #[command(aliases = ["edit", "update"])]
    Set(Box<SetArgs>),

    /// Search task names, notes, projects, and IDs for the given terms.
    /// Equivalent to typing the same terms positionally — e.g.
    /// `tasks search lamaze classes` ≡ `tasks lamaze classes`.
    Search(SearchArgs),

    /// Delete one task. Deleting a **habit** destroys its whole recurring
    /// chain, so that needs the explicit `--habit` opt-in — which is what keeps
    /// a task-cleanup pass structurally unable to reach one.
    #[command(aliases = ["rm", "drop"])]
    Remove(RemoveArgs),

    /// Push a task's due date out, with the defer penalty: `defer_count`
    /// climbs, the `mit` tag is shed, and a `p0` drops to `p1`. A task that is
    /// waiting or blocked defers for free, as does `--no-count`.
    Defer(DeferArgs),

    /// Bump a task or habit's `last_touched` to today and change nothing else
    /// — the "yes, I still care, leave it" acknowledgement.
    Touch(TouchArgs),

    /// Assign a task or habit to another portable workspace member.
    Assign(AssignArgs),

    /// List chronically-ignored tasks: not done, deadline imminent or absent,
    /// and stale, stuck in progress, or captured-and-forgotten. The deadwood
    /// sweep triage runs, as deterministic calendar maths rather than prose.
    Chronic(ScanArgs),

    /// List tasks stuck in `waiting` longer than the threshold — paused on
    /// someone else for long enough that chasing is the right move. A row with
    /// no `waiting_since` is surfaced regardless.
    #[command(name = "stale-waiting", alias = "waiting")]
    StaleWaiting(WaitingArgs),

    /// List tasks carrying an external issue-tracker link. Brain never contacts
    /// the tracker; this is the read a caller reconciles from.
    Linked(LinkedArgs),

    /// Bake caller-supplied markdown into the day's agenda as one generic
    /// appendix section. Re-running replaces it rather than duplicating.
    #[command(name = "agenda-appendix")]
    AgendaAppendix(AgendaAppendixArgs),

    /// Re-sync the day's agenda markdown after a task/habit mutation: drop the
    /// id from the MIT callout / Suggested order / Cut order and re-derive
    /// Today's habits + Completed today from the CSVs, leaving every other
    /// section byte-for-byte. Idempotent; a no-op when the date has no agenda.
    #[command(name = "sync-agenda")]
    SyncAgenda(SyncAgendaArgs),

    /// Validate selected-workspace requirements, session DB, and Claude/Codex
    /// hooks. Exits 0 when required agent-session checks pass.
    Doctor,
}

#[derive(Args, Debug)]
pub struct AddArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long = "type")]
    pub task_type: Option<String>,
    #[arg(long)]
    pub priority: String,
    #[arg(long)]
    pub due: Option<String>,
    #[arg(long)]
    pub start: Option<String>,
    #[arg(long)]
    pub hard_deadline: bool,
    #[arg(long)]
    pub see_also: Option<String>,
    #[arg(long)]
    pub notes: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub energy: Option<String>,
    #[arg(long)]
    pub context: Option<String>,
    #[arg(long)]
    pub duration: Option<String>,
    #[arg(long)]
    pub blocked_by: Option<String>,
    #[arg(long)]
    pub assigned_to: Option<String>,
    #[arg(long)]
    pub linear_issue: Option<String>,
    #[arg(long)]
    pub habit: bool,
    #[arg(long)]
    pub interval: Option<u32>,
    #[arg(long)]
    pub unit: Option<String>,
    /// Time of day a habit belongs to ("6:45 AM"). Habits only; drives the
    /// Morning/Afternoon/Evening grouping in the habits views.
    #[arg(long)]
    pub ideal_time: Option<String>,
    #[arg(long)]
    pub chunks: Option<u32>,
    /// Emit one JSON object describing all created rows.
    #[arg(long)]
    pub json: bool,
}

/// `tasks set <id>` — every mirrored property, plus the habit opt-in.
///
/// Names deliberately match `tasks add` (`--name`, `--due`, `--priority`, …) so
/// one mental model covers create and edit. Omitting every field drops a human
/// into an interactive field picker; an agent always passes flags.
#[derive(Args, Debug)]
pub struct SetArgs {
    /// Task or habit to edit: t123, T123, 123, h43, H43, or a unique fuzzy name.
    pub id: String,

    /// New title.
    #[arg(long)]
    pub name: Option<String>,

    /// New due date: YYYY-MM-DD, `today`, `tomorrow`, or empty to clear it.
    #[arg(long)]
    pub due: Option<String>,

    /// New priority (p0..p4).
    #[arg(long)]
    pub priority: Option<String>,

    /// New status (not_started, in_progress, waiting, done, backlog).
    #[arg(long)]
    pub status: Option<String>,

    /// Replace the notes field.
    #[arg(long)]
    pub notes: Option<String>,

    /// Move the task to a project slug (empty to unlink).
    #[arg(long)]
    pub project: Option<String>,

    /// Attach or repoint the mirrored issue identifier (e.g. AVA-123).
    #[arg(long)]
    pub linear_issue: Option<String>,

    /// New estimated duration in minutes.
    #[arg(long)]
    pub duration: Option<String>,

    /// New time-of-day slot for a habit ("6:45 AM"). Habits only.
    #[arg(long)]
    pub ideal_time: Option<String>,

    /// Required to edit a habit row, and refused for a task.
    #[arg(long)]
    pub habit: bool,

    /// Emit one JSON object describing the applied changes.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct CompleteArgs {
    /// Task or habit ID: t123, T123, 123 (assumed task), h43, H43.
    pub id: String,
}

#[derive(Args, Debug)]
pub struct ScanArgs {
    /// Emit one JSON object per hit instead of a table.
    #[arg(long)]
    pub json: bool,

    /// Print only the number of hits.
    #[arg(long)]
    pub count: bool,
}

#[derive(Args, Debug)]
pub struct WaitingArgs {
    #[command(flatten)]
    pub scan: ScanArgs,

    /// Days waiting before a row is flagged.
    #[arg(long, default_value_t = 7)]
    pub threshold: i64,
}

#[derive(Args, Debug)]
pub struct LinkedArgs {
    #[command(flatten)]
    pub scan: ScanArgs,

    /// Only rows whose status is not `done`.
    #[arg(long)]
    pub open_only: bool,
}

#[derive(Args, Debug)]
pub struct AgendaAppendixArgs {
    /// Markdown file whose contents become the appendix.
    #[arg(long, value_name = "PATH")]
    pub content: std::path::PathBuf,

    /// The agenda's date. Defaults to today.
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub date: Option<String>,
}

#[derive(Args, Debug)]
pub struct RemoveArgs {
    /// Task or habit ID: t123, T123, 123, h43, H43, or a unique name fragment.
    pub id: String,

    /// Required to remove a habit, and refused for a task.
    #[arg(long)]
    pub habit: bool,
}

#[derive(Args, Debug)]
pub struct DeferArgs {
    /// Task ID: t123, T123, 123, or a unique name fragment.
    pub id: String,

    /// `+Nd` to push N days past its current due date, or an absolute
    /// `YYYY-MM-DD`.
    pub when: String,

    /// Defer without the penalty, for a push that genuinely is not the user's
    /// slip. Applied automatically when the task is `waiting` or `blocked_by`
    /// another task.
    #[arg(long)]
    pub no_count: bool,
}

#[derive(Args, Debug)]
pub struct TouchArgs {
    /// Task or habit ID, or a unique name fragment.
    pub id: String,
}

#[derive(Args, Debug)]
pub struct AssignArgs {
    /// Task or habit ID, or a unique name fragment.
    pub id: String,

    /// Portable workspace user ID (lower-case kebab).
    pub user: String,
}

/// What happened to the mutated row, which decides whether the actionable
/// sections are edited at all.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgendaAction {
    /// Completed: drop it, handing a chunked task's slot to its next chunk.
    Done,
    /// Moved off today: drop it.
    Defer,
    /// Still on today's plan: refresh only the CSV-derived snapshots.
    #[default]
    Touch,
}

#[derive(Args, Debug)]
pub struct SyncAgendaArgs {
    /// Task or habit ID the mutation touched: t123, T123, 123, h43, H43.
    /// Omit it to only re-derive the snapshot sections from the CSVs.
    pub id: Option<String>,

    /// What happened to that row. Defaults to `touch`, which never edits the
    /// plan — so a caller that forgets it can't drop a line by accident.
    #[arg(long, value_enum, default_value_t = AgendaAction::Touch)]
    pub action: AgendaAction,

    /// The agenda's date. Defaults to today.
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub date: Option<String>,
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

    /// Filter by portable workspace user ID.
    #[arg(long, value_name = "USER_ID", global = true)]
    pub assigned_to: Option<String>,

    /// Filter by mirrored issue-tracker identifier (e.g. AVA-123).
    /// Case-insensitive exact match on the `linear_issue` column — the lookup
    /// for "which local task mirrors this issue?".
    #[arg(long, value_name = "ISSUE", global = true)]
    pub linear_issue: Option<String>,

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
    use super::{Cli, Command};
    use clap::{CommandFactory, Parser};

    #[test]
    fn help_describes_tasks_under_the_selected_workspace() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("selected workspace"), "{help}");
        assert!(!help.contains("~/brain/tasks"), "{help}");
    }

    #[test]
    fn task_creation_options_are_a_supported_subcommand() {
        let parsed = Cli::try_parse_from([
            "tasks",
            "add",
            "--name",
            "Follow up",
            "--type",
            "ceo",
            "--priority",
            "p1",
            "--due",
            "2026-08-10",
            "--notes",
            "from email",
        ])
        .expect("tasks add should parse");

        assert!(matches!(parsed.command, Some(Command::Add(_))));
    }

    #[test]
    fn task_creation_supports_json_output() {
        let parsed = Cli::try_parse_from([
            "tasks",
            "add",
            "--name",
            "Follow up",
            "--type",
            "personal|needs_attention",
            "--priority",
            "p1",
            "--json",
        ])
        .expect("tasks add --json should parse");
        let Some(Command::Add(args)) = parsed.command else {
            panic!("expected add");
        };
        assert!(args.json);
    }
}
