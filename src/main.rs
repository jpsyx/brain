//! `brain` — the central terminal dispatch for the user's second brain and
//! task system.
//!
//! `brain` is the one command to reach everything the user does from the
//! terminal around `~/brain`: cd between PARA buckets, fuzzy-pick a note
//! across them, hand a prompt to claude, or jump straight into the `tasks`
//! TUI (task management, agenda, triage). Bare `brain` opens a menu of all
//! of these.
//!
//! Layout (mirrors `tasks/`):
//!   - `cli`         — clap surface (Cli + Cmd)
//!   - `plan`        — emit shell-side directives (`cd=`, `claude=`, `tasks=`…)
//!   - `entry`       — directory walker (walkdir + hidden-file filter)
//!   - `picker`      — ratatui fuzzy-picker over collected entries
//!   - `menu`        — ratatui top-level menu (shown on bare `brain`)
//!   - `render`      — palette + styled line helpers for the picker
//!   - `paths`       — brain-root resolution (config.json / $HOME)
//!   - `open_target` — pure "how to open this path" decisions
//!
//! Why the binary doesn't `cd`, call `cl`, or run `tasks` itself:
//!   Those effects need the parent zsh shell (cd mutates the caller's CWD;
//!   `cl` and `tasks` are zsh functions/aliases, not binaries on PATH). So
//!   this process prints a tiny plan to stdout and the wrapper executes it.
//!   Interactive work (TUI, opening Finder) is fully self-contained here,
//!   with the TUI rendering to `/dev/tty` so the wrapper can capture stdout
//!   cleanly.

mod cli;
mod config;
mod confirm;
mod entry;
mod main_view;
mod menu;
mod open_target;
mod paths;
mod picker;
mod plan;
mod pty_pane;
mod render;
mod session;
mod state;
mod tasks;
mod tui;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};
use chrono::{Local, NaiveDate};
use clap::Parser;

use crate::cli::{Cli, Cmd, QueryArgs};
use crate::entry::Bucket;
use crate::menu::Choice;
use crate::picker::{Outcome, Selection};
use crate::tasks::cli::{Cli as TasksCli, Command as TasksCommand};
use crate::tasks::selector::{Selector, parse_selector};
use crate::tasks::view::View;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let brain = paths::brain_root()?;

    match cli.command {
        // Bare `brain` opens the merged persistent shell in its default
        // (tasks) view with the brain panel already open. `brain <freeform>`
        // stays a one-shot global note search (fast "find a note" flow).
        None if cli.args.is_empty() => tasks_launch(TasksCli::parse_from(["brain"])),
        None => search(&brain, &all_buckets(&brain), &cli.args.join(" ")),
        Some(Cmd::Pr(args)) => bucket(&brain, Bucket::Projects, &brain.join("projects"), &args),
        Some(Cmd::Ar(args)) => bucket(&brain, Bucket::Areas, &brain.join("areas"), &args),
        Some(Cmd::Re(args)) => bucket(&brain, Bucket::Resources, &brain.join("resources"), &args),
        Some(Cmd::S(args)) => search(&brain, &all_buckets(&brain), &args.query.join(" ")),
        Some(Cmd::Cd) => {
            plan::cd(&brain);
            Ok(())
        }
        Some(Cmd::Msg(args)) => {
            plan::claude(&brain, &args.query.join(" "));
            Ok(())
        }
        // `brain tasks …` — delegate everything after `tasks` to the tasks
        // CLI parser (positional view/date/search, filter flags, and the
        // complete / doctor / search subcommands), after the natural-language
        // `mark …` rewrite.
        Some(Cmd::Tasks(args)) => {
            let rewritten = rewrite_mark_grammar(
                std::iter::once("brain tasks".to_owned())
                    .chain(args.rest)
                    .collect(),
            );
            tasks_launch(TasksCli::parse_from(rewritten))
        }
    }
}

/// Run the action behind a command-palette choice. The palette itself is a
/// modal overlay inside the picker (`Ctrl-p`); the picker hands back the
/// confirmed `Choice` as `Outcome::Choice`, and dismissing the overlay with
/// Esc never reaches here.
fn dispatch(brain: &Path, choice: Choice) -> Result<()> {
    match choice {
        // "Open tasks" from the one-shot picker launches the merged shell in
        // its default (tasks) view — the old cross-process `tasks` handoff is
        // gone now that tasks is a main view of this binary.
        Choice::OpenTasks => tasks_launch(TasksCli::parse_from(["brain"])),
        Choice::SearchProjects => {
            search(brain, &[(Bucket::Projects, brain.join("projects"))], "")
        }
        Choice::SearchAreas => search(brain, &[(Bucket::Areas, brain.join("areas"))], ""),
        Choice::SearchResources => {
            search(brain, &[(Bucket::Resources, brain.join("resources"))], "")
        }
        Choice::SearchArchive => {
            search(brain, &[(Bucket::Archive, brain.join("archive"))], "")
        }
        Choice::GlobalSearch => search(brain, &all_buckets(brain), ""),
        Choice::Msg => {
            plan::claude(brain, "");
            Ok(())
        }
        // Layout-swap only means something in the persistent shell (handled
        // there); from the one-shot picker it's a no-op. The other conditional
        // rows never reach `dispatch`: the picker resolves "Create PDF" to
        // `Outcome::CreatePdf(path)`, "Open file" / "Open directory" to an
        // `Outcome::Selected` (Open / Reveal), and handles "Delete" inline via
        // the confirmation modal — all needing the highlighted path.
        Choice::ToggleLayout
        | Choice::CreatePdf
        | Choice::OpenFile
        | Choice::OpenDir
        | Choice::Delete => Ok(()),
    }
}

/// Launch (or dispatch a utility for) the tasks view of the merged shell.
///
/// Ported from the old `tasks` binary's `main`/`browse`: the `complete` /
/// `doctor` / `search` subcommands are one-shot utilities; everything else
/// resolves an initial view and opens the persistent shell via `tui::run_tui`.
fn tasks_launch(mut cli: TasksCli) -> Result<()> {
    let today = Local::now().date_naive();
    let initial = match cli.command.take() {
        Some(TasksCommand::Complete(args)) => return tasks::complete::run(&args.id),
        Some(TasksCommand::Search(args)) => Initial::CustomSearch(args.query.join(" ")),
        Some(TasksCommand::Doctor) => {
            let db_path = state::Db::default_path();
            let settings_dir = std::env::var_os("HOME").map_or_else(|| PathBuf::from(".claude"), |h| PathBuf::from(h).join("brain").join(".claude"));
            let diag = tasks::doctor::run_doctor(&db_path, &settings_dir);
            std::process::exit(tasks::doctor::print_report(&diag));
        }
        None => resolve_query(&cli.query, today),
    };
    tasks_browse(initial, &mut cli, today)
}

/// What the tasks positional input resolves to (mirrors the old `tasks`
/// binary's `Initial`).
enum Initial {
    View(View),
    Custom(Selector),
    CustomSearch(String),
}

fn resolve_query(tokens: &[String], today: NaiveDate) -> Initial {
    if tokens.is_empty() {
        return Initial::View(View::Today);
    }
    if tokens.len() == 1 {
        if let Some(view) = View::from_token(&tokens[0]) {
            return Initial::View(view);
        }
        if let Ok(sel) = parse_selector(Some(&tokens[0]), today) {
            return Initial::Custom(sel);
        }
    }
    Initial::CustomSearch(tokens.join(" "))
}

fn tasks_browse(initial: Initial, cli: &mut TasksCli, today: NaiveDate) -> Result<()> {
    let csv_path = cli.csv.clone().unwrap_or_else(default_csv_path);
    let all_tasks = tasks::task::load_tasks(&csv_path)?;
    let habits_path = csv_path.with_file_name("habits.csv");
    let habits = tasks::task::load_habits(&habits_path).unwrap_or_default();

    let (selector, start_view, initial_search) = match initial {
        Initial::View(v) => (v.selector(today), Some(v), None),
        Initial::Custom(sel) => (sel, None, None),
        Initial::CustomSearch(q) => (Selector::All, Some(View::All), Some(q)),
    };

    if cli.display.no_tui {
        if let Some(q) = &initial_search {
            cli.filters.search = Some(q.clone());
        }
    }

    let initial_data = if start_view == Some(View::Habits) {
        habits.clone()
    } else {
        all_tasks.clone()
    };
    let view = tasks::view::build_view(cli, &selector, start_view, initial_data, today);

    if cli.display.no_tui {
        tasks::plain::print_plain(&view, today, cli.display.full_notes);
    } else {
        tui::run_tui(
            &view,
            cli,
            today,
            csv_path,
            all_tasks,
            habits,
            start_view,
            initial_search,
        )?;
    }
    Ok(())
}

fn default_csv_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_owned());
    PathBuf::from(home).join("brain/tasks/tasks.csv")
}

/// Translate natural-language completion grammar into `complete <ID>` before
/// clap sees it. `brain tasks mark t1 [as] done` → `brain tasks complete t1`.
/// argv[0] here is the synthetic `"brain tasks"`; the `mark` keyword is at
/// index 1. Unrecognized trailing words are left alone for the search path.
fn rewrite_mark_grammar(args: Vec<String>) -> Vec<String> {
    if args.len() < 3 || !args[1].eq_ignore_ascii_case("mark") {
        return args;
    }
    let id_pos = 2;
    let after_id = &args[id_pos + 1..];
    let consume = match after_id {
        [a, b, ..] if a.eq_ignore_ascii_case("as") && b.eq_ignore_ascii_case("done") => 2,
        [a, ..] if a.eq_ignore_ascii_case("done") => 1,
        [] => 0,
        [first, ..] if first.starts_with('-') => 0,
        _ => return args,
    };
    let mut out = Vec::with_capacity(args.len());
    out.push(args[0].clone());
    out.push("complete".to_owned());
    out.push(args[id_pos].clone());
    out.extend_from_slice(&args[id_pos + 1 + consume..]);
    out
}

fn all_buckets(brain: &Path) -> Vec<(Bucket, PathBuf)> {
    vec![
        (Bucket::Projects, brain.join("projects")),
        (Bucket::Areas, brain.join("areas")),
        (Bucket::Resources, brain.join("resources")),
        (Bucket::Archive, brain.join("archive")),
    ]
}

fn bucket(brain: &Path, bucket: Bucket, dir: &Path, args: &QueryArgs) -> Result<()> {
    if args.query.is_empty() {
        plan::cd(dir);
        return Ok(());
    }
    search(brain, &[(bucket, dir.to_path_buf())], &args.query.join(" "))
}

fn search(brain: &Path, roots: &[(Bucket, PathBuf)], query: &str) -> Result<()> {
    let entries = entry::collect(brain, roots)?;
    match picker::run(&entries, query)? {
        None => Ok(()),
        Some(Outcome::Selected(Selection::Reveal(path))) => open_in_finder(&path),
        Some(Outcome::Selected(Selection::Open(path))) => open_directly(&path),
        // The user confirmed a command-palette row (Ctrl-p → Enter).
        Some(Outcome::Choice(choice)) => dispatch(brain, choice),
        // Convert a markdown file to a colocated PDF, then open it.
        Some(Outcome::CreatePdf(path)) => create_pdf_and_open(&path),
    }
}

/// Build the colocated PDF for a markdown file and hand it to the wrapper's
/// `open=` directive so the parent shell opens it after `brain` exits.
fn create_pdf_and_open(md: &Path) -> Result<()> {
    let pdf = open_target::create_pdf(md)?;
    plan::open(&pdf);
    Ok(())
}

fn open_in_finder(path: &Path) -> Result<()> {
    let target = open_target::finder_target(path, path.is_file());
    let status = Command::new("open").arg(target).status()?;
    if !status.success() {
        bail!("open exited with status {status}");
    }
    // Also hand the directory to the wrapper so the parent shell ends up
    // cd'd there after `brain` exits.
    plan::cd(target);
    Ok(())
}

/// Ctrl-/Cmd-Enter path: open the selection itself. Directories still get
/// revealed in Finder (no useful "open dir in editor" behavior). Text-like
/// files hand off to the wrapper's `edit=` directive so the user's
/// configured editor runs in the existing terminal. Everything else is
/// handed to the system `open` so the OS picks the default app.
fn open_directly(path: &Path) -> Result<()> {
    if path.is_dir() {
        return open_in_finder(path);
    }
    if let Some(parent) = path.parent() {
        plan::cd(parent);
    }
    if open_target::is_textlike(path) {
        plan::edit(path);
    } else {
        plan::open(path);
    }
    Ok(())
}
