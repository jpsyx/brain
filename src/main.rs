//! `brain` — the central terminal dispatch for the user's second brain and
//! task system.
//!
//! `brain` opens a persistent shell (`tui/`) with two main views: the tasks
//! view (task management, agenda, triage; the startup default) and the
//! brain-directory search view (fuzzy-pick over `~/brain`), plus an app-level
//! brain panel (an interactive `claude` session in a PTY). Everything the user
//! does happens inside that shell; there are no shell-mutating one-shot
//! subcommands, so the binary needs no wrapper. It is exec'd directly.
//!
//! Layout (mirrors `tasks/`):
//!   - `cli`         — clap surface (Cli + Cmd)
//!   - `entry`       — directory walker (walkdir + hidden-file filter)
//!   - `picker`      — ratatui fuzzy-picker over collected entries
//!   - `menu`        — command-palette modal shared by the search view
//!   - `render`      — palette + styled line helpers for the picker
//!   - `paths`       — brain-root resolution (config.json / $HOME)
//!   - `open_target` — pure "how to open this path" decisions
//!
//! The TUI renders to `/dev/tty`; the binary's stdout carries only the small
//! amount of text emitted by `brain config` (and clap's help/errors).

mod cli;
mod config;
mod confirm;
mod entry;
mod main_view;
mod menu;
mod open_target;
mod paths;
mod picker;
mod pty_pane;
mod render;
mod session;
mod settings;
mod state;
mod tasks;
mod tui;

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use chrono::{Local, NaiveDate};
use clap::Parser;

use crate::cli::{Cli, Cmd, ConfigAction, ConfigArgs};
use crate::tasks::cli::{Cli as TasksCli, Command as TasksCommand};
use crate::tasks::selector::{Selector, parse_selector};
use crate::tasks::view::View;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // `brain config …` manages the store itself, so it must run *before* the
    // prerequisite gate — otherwise you could never `config set` your way out
    // of a missing `markdown-to-pdf`.
    if let Some(Cmd::Config(args)) = &cli.command {
        return config_command(args);
    }

    // `markdown-to-pdf` is a hard prerequisite (brain runs it for the
    // Create-PDF flow). Its path is a config variable, auto-discovered and
    // persisted on first run; fail fast with a helpful message if it can't be
    // resolved. Runs after clap has handled `--help`/`--version`.
    settings::ensure_markdown_to_pdf();

    match cli.command {
        // Bare `brain` opens the merged persistent shell in its default
        // (tasks) view with the brain panel already open.
        None => tasks_launch(TasksCli::parse_from(["brain"])),
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
        // Handled before the prerequisite gate above.
        Some(Cmd::Config(_)) => unreachable!("config is dispatched before the gate"),
    }
}

/// Handle `brain config {list|get|set}`. Output goes to stdout; `get` on an
/// unset variable notes so on stderr. Bare `brain config` lists.
fn config_command(args: &ConfigArgs) -> Result<()> {
    match args.action.as_ref().unwrap_or(&ConfigAction::List) {
        ConfigAction::List => {
            print!(
                "{}",
                settings::render_list(&settings::resolve_all(), settings::color_enabled())
            );
        }
        ConfigAction::Get { name } => {
            let name = settings::normalize_name(name);
            match settings::resolve_one(&name) {
                Some(v) => println!("{v}"),
                None => eprintln!("{name} is unset"),
            }
        }
        ConfigAction::Set { assignment } => {
            let (raw_name, value) = assignment
                .split_once('=')
                .ok_or_else(|| anyhow!("expected name=value, got `{assignment}`"))?;
            let (name, value) = (settings::normalize_name(raw_name), value.trim());
            settings::set(&name, value)?;
            println!(
                "{}",
                settings::set_confirmation(&name, value, settings::color_enabled())
            );
        }
    }
    Ok(())
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
