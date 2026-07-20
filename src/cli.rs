//! Clap surface.
//!
//! Subcommands: `pr|project|projects`, `ar|area|areas`, `re|resource|resources`,
//! `s|search`, `cd`, `msg`, and `tasks`. Each bucket / query command takes an
//! optional query; `msg` takes a message; `cd` and `tasks` take none.
//!
//! When no subcommand is given:
//!   - empty args → show the interactive top-level menu
//!   - otherwise  → fuzzy-search across all buckets (equivalent to `s`)

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "brain",
    version,
    about = "Brain CLI: central terminal dispatch for ~/brain and the task system.",
    long_about = "Brain CLI: the central terminal dispatch for the user's second\n\
                  brain and task system. One command to cd between PARA buckets,\n\
                  fuzzy-pick a note, chat with claude, or jump into the tasks TUI.\n\
                  \n\
                  Subcommands:\n\
                  \n\
                    pr (project, projects)     ~/brain/projects bucket\n\
                    ar (area, areas)           ~/brain/areas bucket\n\
                    re (resource, resources)   ~/brain/resources bucket\n\
                      With no further args: cd into the bucket.\n\
                      With a query: open a fuzzy picker scoped to the bucket.\n\
                  \n\
                    s (search) [query]         Fuzzy-pick across projects,\n\
                                               areas, and resources together.\n\
                                               With no query the picker opens\n\
                                               empty.\n\
                  \n\
                    cd                         cd into the brain root\n\
                                               (configured via `brain config`).\n\
                  \n\
                    msg <prompt>               cd into ~/brain and hand <prompt>\n\
                                               to claude (via the `cl` alias)\n\
                                               as the opening message.\n\
                  \n\
                    tasks                      Open the tasks TUI (task\n\
                                               management, agenda, triage) by\n\
                                               running the `tasks` command.\n\
                  \n\
                  With no subcommand: empty args open global search\n\
                  directly; any other args run a search across all\n\
                  buckets (equivalent to `s`). Ctrl-p opens the command\n\
                  palette (every action brain can run).\n\
                  \n\
                  TUI keys: type to filter · ↑/↓ or Ctrl-k/Ctrl-n\n\
                  move · Enter open file (text → $EDITOR, otherwise\n\
                  system open) · Ctrl-Enter reveal in Finder · Ctrl-p\n\
                  palette · Esc / Ctrl-c quit."
)]
pub struct Cli {
    /// Free-form positional input. If the first word matches a subcommand,
    /// dispatch to it. Otherwise, the entire args become the search query
    /// (equivalent to `brain search <args>`).
    pub args: Vec<String>,

    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Projects bucket. No args → cd. With a query → fuzzy-pick within it.
    #[command(visible_aliases = ["project", "projects"])]
    Pr(QueryArgs),

    /// Areas bucket. No args → cd. With a query → fuzzy-pick within it.
    #[command(visible_aliases = ["area", "areas"])]
    Ar(QueryArgs),

    /// Resources bucket. No args → cd. With a query → fuzzy-pick within it.
    #[command(visible_aliases = ["resource", "resources"])]
    Re(QueryArgs),

    /// Fuzzy-pick across projects, areas, and resources simultaneously.
    #[command(visible_alias = "search")]
    S(QueryArgs),

    /// cd into the brain root (path configured via `brain config`).
    Cd,

    /// Hand the message to claude (via the `cl` alias) as the opening prompt.
    Msg(QueryArgs),

    /// Open the merged shell in the tasks view, or run a tasks utility.
    ///
    /// All arguments after `tasks` are delegated verbatim to the tasks CLI
    /// parser, so `brain tasks`, `brain tasks today --no-tui`,
    /// `brain tasks complete t123`, `brain tasks doctor`, and
    /// `brain tasks search lamaze` all work. Bare `brain` is equivalent to
    /// `brain tasks` (the tasks view is the startup default).
    Tasks(TasksArgs),

    /// Read or change brain's persistent config (`~/.config/brain/config.json`).
    Config(ConfigArgs),
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
pub struct TasksArgs {
    /// Everything after `tasks`, handed to the tasks CLI parser unchanged
    /// (positional view/date/search tokens, filter flags, and the
    /// `complete` / `doctor` / `search` subcommands).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

#[derive(Args, Debug)]
pub struct QueryArgs {
    /// Words are joined with spaces and used as the initial picker query.
    pub query: Vec<String>,
}
