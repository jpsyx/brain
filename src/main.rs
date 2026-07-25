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
mod env;
mod main_view;
mod menu;
mod open_target;
mod paths;
mod personalization;
mod picker;
mod pty_pane;
mod render;
mod session;
mod settings;
mod skills;
mod state;
mod sync;
mod tasks;
mod theme;
mod tui;

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use chrono::{Local, NaiveDate};
use clap::Parser;

use crate::cli::{
    Cli, Cmd, ConfigAction, ConfigArgs, EnvAction, EnvArgs, PersonalizeAction, PersonalizeArgs,
    SyncAction, SyncArgs,
};
use crate::tasks::cli::{Cli as TasksCli, Command as TasksCommand};
use crate::tasks::selector::{Selector, parse_selector};
use crate::tasks::view::View;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load the user's tag styles once for this process so the task renderer can
    // resolve tag labels without threading state through every signature. Cheap,
    // never fails (missing/broken store → generic defaults), so it is safe on
    // every entry path including `config` / `personalize` and `--no-tui`.
    personalization::init_tag_styles();

    // One-time, idempotent migration into brain env (fold the brain-root pointer
    // into env.root; relocate markdown_to_pdf_path from brain config). Never fatal.
    env::migrate();

    // `brain config …` manages the store itself, so it must run *before* the
    // prerequisite gate — otherwise you could never `config set` your way out
    // of a missing `markdown-to-pdf`.
    if let Some(Cmd::Config(args)) = &cli.command {
        return config_command(args);
    }

    // `brain env` manages the machine-local env store; like `config`, it runs
    // before the prerequisite gate so you can repair a broken environment.
    if let Some(Cmd::Env(args)) = &cli.command {
        return env_command(args);
    }

    // `brain sync` needs neither the markdown-to-pdf prerequisite nor the TUI.
    if let Some(Cmd::Sync(args)) = &cli.command {
        return sync_command(args);
    }

    // Like `config`, personalization manages the user's own setup, so it runs
    // before the prerequisite gate.
    if let Some(Cmd::Personalize(args)) = &cli.command {
        return personalize_command(args);
    }

    // `skills` manages the skill install; also before the gate.
    if let Some(Cmd::Skills(args)) = &cli.command {
        return skills_command(args);
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
        Some(Cmd::Env(_)) => unreachable!("env is dispatched before the gate"),
        Some(Cmd::Sync(_)) => unreachable!("sync is dispatched before the gate"),
        Some(Cmd::Personalize(_)) => unreachable!("personalize is dispatched before the gate"),
        Some(Cmd::Skills(_)) => unreachable!("skills is dispatched before the gate"),
    }
}

/// Handle `brain skills {sync}`. Bare `brain skills` defaults to `sync`.
fn skills_command(args: &crate::cli::SkillsArgs) -> Result<()> {
    let root = match &args.action {
        Some(crate::cli::SkillsAction::Sync { root }) => root.as_deref(),
        None => None,
    };
    skills::command::run_sync(root)
}

/// Handle `brain personalize {show|get|set|edit}`. Bare `brain personalize`
/// runs first-run onboarding when nothing is set, otherwise shows current
/// values. Every mutating path re-renders the installed skills.
fn personalize_command(args: &PersonalizeArgs) -> Result<()> {
    match args.action.as_ref() {
        Some(PersonalizeAction::Show) => {
            personalization::command::run_show();
            Ok(())
        }
        Some(PersonalizeAction::Get { field }) => {
            personalization::command::run_get(field);
            Ok(())
        }
        Some(PersonalizeAction::Set { assignment }) => personalization::command::run_set(assignment),
        Some(PersonalizeAction::Edit) => personalization::command::run_edit(),
        None => personalization::onboarding::run_or_show(),
    }
}

/// Handle `brain config {list|get|set}`. Output goes to stdout; `get` on an
/// unset variable notes so on stderr. Bare `brain config` lists.
fn config_command(args: &ConfigArgs) -> Result<()> {
    match args.action.as_ref().unwrap_or(&ConfigAction::List) {
        ConfigAction::List => {
            print!("{}", settings::render_list(&settings::resolve_all(), theme::Theme::active()));
        }
        ConfigAction::Get { name } => {
            let name = settings::normalize_name(name);
            match settings::resolve_one(&name) {
                Some(v) => println!("{v}"),
                None => eprintln!("{name} is unset"),
            }
        }
        ConfigAction::Set { assignment } => {
            if let Some((raw_name, value)) = assignment.split_once('=') {
                // Non-interactive: `name=value`.
                let (name, value) = (settings::normalize_name(raw_name), value.trim());
                settings::set(&name, value)?;
                // Any config change re-renders the installed skills so they
                // never drift from the user's values.
                skills::resync_skills();
                println!("{}", settings::set_confirmation(&name, value, theme::Theme::active()));
            } else {
                // Interactive: bare `name` with no value.
                config_set_interactive(&settings::normalize_name(assignment))?;
            }
        }
    }
    Ok(())
}

/// Handle `brain sync [--push|--pull] {setup|init|status|conflicts}`.
fn sync_command(args: &SyncArgs) -> Result<()> {
    use crate::sync::args::Direction;
    let cfg = crate::sync::config::SyncConfig::load();
    let root = paths::brain_root()?;
    match &args.action {
        Some(SyncAction::Setup) => crate::sync::setup::run(),
        Some(SyncAction::Init) => run_sync(&cfg, &root, Direction::Resync),
        Some(SyncAction::Status) => crate::sync::command::print_status(&cfg, &root),
        Some(SyncAction::Conflicts) => {
            crate::sync::command::print_conflicts(&root);
            Ok(())
        }
        None => {
            let dir = crate::sync::command::direction_from_flags(args.push, args.pull)?;
            run_sync(&cfg, &root, dir)
        }
    }
}

/// Shared: run one sync and print the outcome.
fn run_sync(
    cfg: &crate::sync::config::SyncConfig,
    root: &std::path::Path,
    dir: crate::sync::args::Direction,
) -> Result<()> {
    if !cfg.is_configured() {
        println!("sync is not configured — run `brain sync setup`.");
        return Ok(());
    }
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    let outcome = crate::sync::command::sync_once(cfg, root, dir, (&ts, &ts, &date))?;
    match outcome {
        crate::sync::verify::Outcome::Clean => println!("sync complete."),
        crate::sync::verify::Outcome::NeedsAttention(m) | crate::sync::verify::Outcome::Aborted(m) => {
            eprintln!("{m}");
        }
    }
    Ok(())
}

/// Handle `brain env {list|get|set}`. Output goes to stdout; `get` on an
/// unset variable notes so on stderr. Bare `brain env` lists.
fn env_command(args: &EnvArgs) -> Result<()> {
    match args.action.as_ref().unwrap_or(&EnvAction::List) {
        EnvAction::List => {
            println!("{}", settings::render_list(&env::resolve_all(), theme::Theme::active()));
        }
        EnvAction::Get { name } => {
            let name = settings::normalize_name(name);
            match env::resolve_one(&name) {
                Some(v) => println!("{v}"),
                None => eprintln!("{name} is unset"),
            }
        }
        EnvAction::Set { assignment } => {
            if let Some((name, value)) = assignment.split_once('=') {
                let name = settings::normalize_name(name);
                env::set(&name, value)?;
                println!("{}", settings::set_confirmation(&name, value, theme::Theme::active()));
            } else {
                anyhow::bail!("expected `name=value`, got `{assignment}`");
            }
        }
    }
    Ok(())
}

/// Interactive `brain config set <name>` (no `=value`). `namespaces` and `tags`
/// route to their personalization toggle-checklists; any other variable prompts
/// once on `/dev/tty` for a value, then sets it like the non-interactive path.
fn config_set_interactive(name: &str) -> Result<()> {
    match name {
        "namespaces" => return crate::personalization::command::run_set_namespaces(),
        "tags" | "tag_styles" => return crate::personalization::command::run_set_tags(),
        _ => {}
    }
    let Some(value) = prompt_tty_line(&format!("Set {name} = "))? else {
        // No terminal (headless): can't prompt. Point at the non-interactive form.
        return Err(anyhow!(
            "no terminal for interactive set; use `brain config set {name}=<value>`"
        ));
    };
    let value = value.trim();
    settings::set(name, value)?;
    skills::resync_skills();
    println!("{}", settings::set_confirmation(name, value, theme::Theme::active()));
    Ok(())
}

/// Prompt once on `/dev/tty` and read a line. `Ok(None)` when there is no
/// controlling terminal (so callers can fall back rather than hang).
fn prompt_tty_line(prompt: &str) -> Result<Option<String>> {
    use std::io::{BufRead, BufReader, Write};
    let Ok(tty) = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") else {
        return Ok(None);
    };
    let mut out = tty.try_clone()?;
    let mut reader = BufReader::new(tty);
    write!(out, "{prompt}")?;
    out.flush()?;
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None); // EOF
    }
    Ok(Some(line))
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
    // Honor the configured brain `root`; fall back to `$HOME/brain` when it is
    // unset or missing (mirrors the TUI startup in `tui::event_loop::setup`).
    let root = paths::brain_root().unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_owned());
        PathBuf::from(home).join("brain")
    });
    root.join("tasks").join("tasks.csv")
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
