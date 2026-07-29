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
//!   - `logging`     — optional per-run verbose log file + stdout mirroring
//!   - `picker`      — ratatui fuzzy-picker over collected entries
//!   - `menu`        — command-palette modal shared by the search view
//!   - `render`      — palette + styled line helpers for the picker
//!   - `paths`       — brain-root resolution (config.json / $HOME)
//!   - `open_target` — pure "how to open this path" decisions
//!
//! The TUI renders to `/dev/tty`; the binary's stdout carries only the small
//! amount of text emitted by config/env/version surfaces, explicit verbose logs,
//! and clap's help/errors.

mod cli;
mod config;
mod confirm;
mod entry;
mod env;
mod logging;
mod main_view;
mod menu;
mod open_target;
mod paths;
mod personalization;
mod picker;
mod pty_pane;
mod render;
mod server;
mod session;
mod settings;
mod skills;
mod state;
mod sync;
mod tasks;
mod theme;
mod tui;

use std::path::PathBuf;

use crate::cli::{
    Cmd, ConfigAction, ConfigArgs, EnvAction, EnvArgs, PersonalizeAction, PersonalizeArgs,
    SyncAction, SyncArgs,
};
use crate::tasks::cli::{Cli as TasksCli, Command as TasksCommand};
use crate::tasks::selector::{Selector, parse_selector};
use crate::tasks::view::View;
use anyhow::{Result, anyhow};
use chrono::{Local, NaiveDate};
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::parse();
    let agent_kind = cli.agent_kind();

    if cli.print_version || matches!(&cli.command, Some(Cmd::Version)) {
        print!("{}", crate::cli::version_line());
        return Ok(());
    }

    let _log_guard = logging::init(cli.verbose, true)?;
    logging::log(format!(
        "argv {:?}",
        std::env::args().collect::<Vec<String>>()
    ));

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
        logging::log("dispatch config");
        return config_command(args);
    }

    // `brain env` manages the machine-local env store; like `config`, it runs
    // before the prerequisite gate so you can repair a broken environment.
    if let Some(Cmd::Env(args)) = &cli.command {
        logging::log("dispatch env");
        return env_command(args);
    }

    // `brain sync` needs neither the markdown-to-pdf prerequisite nor the TUI.
    if let Some(Cmd::Sync(args)) = &cli.command {
        logging::log("dispatch sync");
        return sync_command(args);
    }

    // Like `config`, personalization manages the user's own setup, so it runs
    // before the prerequisite gate.
    if let Some(Cmd::Personalize(args)) = &cli.command {
        logging::log("dispatch personalize");
        return personalize_command(args);
    }

    // `skills` manages the skill install; also before the gate.
    if let Some(Cmd::Skills(args)) = &cli.command {
        logging::log("dispatch skills");
        return skills_command(args);
    }

    // `brain server …` manages the background HTTP daemon; it needs neither
    // the markdown-to-pdf prerequisite nor the TUI, so it runs before the gate.
    if let Some(Cmd::Server(args)) = &cli.command {
        logging::log("dispatch server");
        return server_command(args);
    }

    if let Some(Cmd::Receiver(args)) = &cli.command {
        logging::log("dispatch receiver");
        return receiver_server_command(args);
    }

    // `brain habits` just opens the bundled habits page (starting the server if
    // needed); no markdown-to-pdf prerequisite and no TUI, so it runs before the
    // gate.
    if matches!(&cli.command, Some(Cmd::Habits)) {
        logging::log("dispatch habits");
        return habits_command();
    }

    // `brain check` is a read-only report; no TUI, no prerequisite needed.
    if matches!(&cli.command, Some(Cmd::Check)) {
        logging::log("dispatch check");
        let cfg = crate::sync::config::SyncConfig::load();
        let root = paths::brain_root()?;
        crate::sync::check::run(&cfg, &root);
        return Ok(());
    }

    // `markdown-to-pdf` is a hard prerequisite (brain runs it for the
    // Create-PDF flow). Its path is a config variable, auto-discovered and
    // persisted on first run; fail fast with a helpful message if it can't be
    // resolved. Runs after clap has handled `--help`/`--version`.
    settings::ensure_markdown_to_pdf();

    match cli.command {
        // Bare `brain` opens the merged persistent shell in its default
        // (tasks) view with the brain panel already open.
        None => tasks_launch(TasksCli::parse_from(["brain"]), agent_kind, cli.with_receiver),
        // `brain tasks …` — delegate everything after `tasks` to the tasks
        // CLI parser (positional view/date/search, filter flags, and the
        // complete / doctor / search subcommands), after the natural-language
        // `mark …` rewrite.
        Some(Cmd::Tasks(mut args)) => {
            logging::log("dispatch tasks");
            let agent_kind = if take_codex_flag(&mut args.rest) {
                session::AgentKind::Codex
            } else {
                agent_kind
            };
            let rewritten = rewrite_mark_grammar(
                std::iter::once("brain tasks".to_owned())
                    .chain(args.rest)
                    .collect(),
            );
            tasks_launch(TasksCli::parse_from(rewritten), agent_kind, cli.with_receiver)
        }
        Some(Cmd::Version) => unreachable!("version is dispatched before the gate"),
        // Handled before the prerequisite gate above.
        Some(Cmd::Config(_)) => unreachable!("config is dispatched before the gate"),
        Some(Cmd::Env(_)) => unreachable!("env is dispatched before the gate"),
        Some(Cmd::Sync(_)) => unreachable!("sync is dispatched before the gate"),
        Some(Cmd::Personalize(_)) => unreachable!("personalize is dispatched before the gate"),
        Some(Cmd::Skills(_)) => unreachable!("skills is dispatched before the gate"),
        Some(Cmd::Server(_)) => unreachable!("server is dispatched before the gate"),
        Some(Cmd::Receiver(_)) => {
            unreachable!("receiver is dispatched before the gate")
        }
        Some(Cmd::Habits) => unreachable!("habits is dispatched before the gate"),
        Some(Cmd::Check) => unreachable!("check is dispatched before the gate"),
    }
}

fn receiver_server_command(args: &crate::cli::ReceiverArgs) -> Result<()> {
    use crate::cli::ReceiverServerAction;
    if matches!(&args.action, ReceiverServerAction::Setup) {
        return receiver_setup();
    }
    let command = match &args.action {
        ReceiverServerAction::Start => "start",
        ReceiverServerAction::Stop => "stop",
        ReceiverServerAction::Restart => "restart",
        ReceiverServerAction::Status => "status",
        ReceiverServerAction::Logs => "logs",
        ReceiverServerAction::Setup => unreachable!("setup handled above"),
    };
    match crate::server::receiver::send_control(command) {
        Ok(response) => {
            print!("{response}");
            Ok(())
        }
        Err(_) => {
            if matches!(&args.action, ReceiverServerAction::Status) {
                println!("receiver server is stopped (no brain TUI is running)");
                Ok(())
            } else {
                anyhow::bail!(
                    "the receiver server belongs to the running brain TUI; use `brain --with-receiver` or the command palette"
                )
            }
        }
    }
}

fn receiver_setup() -> Result<()> {
    let theme = theme::Theme::active();
    println!("{}", theme.heading("Set up the brain receiver"));
    println!("{}", theme.muted("Choose which channels to configure:"));
    println!("  {}", theme.accent("1) Email"));
    println!("  {}", theme.accent("2) SMS"));
    println!("  {}", theme.accent("3) Both"));
    let Some(channel_input) = prompt_tty_line(&format!("{} ", theme.prompt("Choose 1, 2, or 3:")))? else {
        anyhow::bail!("receiver setup needs an interactive terminal; nothing was changed");
    };
    let Some(channels) = parse_receiver_channels(channel_input.trim()) else {
        anyhow::bail!("choose 1 for email, 2 for SMS, or 3 for both");
    };
    println!(
        "{}",
        theme.muted("Press Enter to keep an existing value. Type /clear to erase it.")
    );
    let current = crate::config::Config::load();
    let prompts = [
        (
            "response_email",
            "Email address for longer SMS replies",
            "When you text the receiver and ask for a reply too long for SMS, Brain sends the full answer here.",
            current.response_email,
        ),
        (
            "allowed_sms_senders",
            "Phone numbers allowed to text Brain (comma-separated)",
            "Messages from any other phone number are rejected before they reach the LLM.",
            current.allowed_sms_senders,
        ),
        (
            "allowed_email_senders",
            "Email addresses allowed to contact Brain (comma-separated)",
            "Only these senders can trigger Brain; replies stay limited to eligible people in the email thread.",
            current.allowed_email_senders,
        ),
    ];
    for (name, label, description, old) in prompts.into_iter().filter(|(name, ..)| match *name {
        "response_email" | "allowed_email_senders" => channels.email(),
        "allowed_sms_senders" => channels.sms(),
        _ => false,
    }) {
        println!("{}", theme.muted(description));
        let hint = if old.trim().is_empty() {
            theme.muted("(not set)")
        } else {
            theme.muted(&format!("(saved: {})", old.trim()))
        };
        let Some(input) = prompt_tty_line(&format!("{} {}: ", theme.prompt(label), hint))? else {
            anyhow::bail!("receiver setup needs an interactive terminal; nothing was changed");
        };
        let value = match input.trim() {
            "" => old,
            "/clear" => String::new(),
            value => value.to_owned(),
        };
        settings::set(name, &value)?;
    }
    install_receiver_hooks(&crate::paths::brain_root()?)?;
    println!("{}", theme.success("receiver configuration saved"));
    println!(
        "{}",
        theme.muted("Provider secrets remain machine-local. Set TWILIO_*/RESEND_* and BRAIN_RECEIVER_PUBLIC_URL before starting the receiver.")
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiverSetupChannels {
    Email,
    Sms,
    Both,
}

impl ReceiverSetupChannels {
    const fn email(self) -> bool {
        matches!(self, Self::Email | Self::Both)
    }

    const fn sms(self) -> bool {
        matches!(self, Self::Sms | Self::Both)
    }
}

fn parse_receiver_channels(input: &str) -> Option<ReceiverSetupChannels> {
    match input {
        "1" => Some(ReceiverSetupChannels::Email),
        "2" => Some(ReceiverSetupChannels::Sms),
        "3" => Some(ReceiverSetupChannels::Both),
        _ => None,
    }
}

fn ensure_hook_entry(settings: &mut serde_json::Value, event: &str, command: &str) {
    let hooks = settings
        .as_object_mut()
        .expect("settings JSON root is an object")
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let events = hooks
        .as_object_mut()
        .expect("hooks JSON is an object")
        .entry(event)
        .or_insert_with(|| serde_json::json!([]));
    let list = events.as_array_mut().expect("hook event is an array");
    let exists = list.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| item.get("command").and_then(serde_json::Value::as_str) == Some(command))
            })
    });
    if !exists {
        list.push(serde_json::json!({
            "hooks": [{"type": "command", "command": command}]
        }));
    }
}

fn install_receiver_hooks(root: &std::path::Path) -> Result<()> {
    let hook_dir = root.join(".claude").join("brain-hooks");
    std::fs::create_dir_all(&hook_dir)?;
    let session_path = hook_dir.join("claude_session_start_hook.py");
    let stop_path = hook_dir.join("claude_stop_hook.py");
    std::fs::write(
        &session_path,
        include_str!("../scripts/claude_session_start_hook.py"),
    )?;
    std::fs::write(&stop_path, include_str!("../scripts/claude_stop_hook.py"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&session_path, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&stop_path, std::fs::Permissions::from_mode(0o755))?;
    }
    let settings_path = root.join(".claude").join("settings.json");
    let mut settings = if settings_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&settings_path)?)?
    } else {
        serde_json::json!({})
    };
    let session = session_path.to_string_lossy().into_owned();
    let stop = stop_path.to_string_lossy().into_owned();
    ensure_hook_entry(&mut settings, "SessionStart", &session);
    ensure_hook_entry(&mut settings, "Stop", &stop);
    std::fs::write(settings_path, serde_json::to_vec_pretty(&settings)?)?;
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    let codex_dir = std::path::PathBuf::from(home).join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    let codex_hooks_path = codex_dir.join("hooks.json");
    let mut codex_hooks = if codex_hooks_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&codex_hooks_path)?)?
    } else {
        serde_json::json!({})
    };
    ensure_hook_entry(&mut codex_hooks, "SessionStart", &session);
    ensure_hook_entry(&mut codex_hooks, "Stop", &stop);
    std::fs::write(codex_hooks_path, serde_json::to_vec_pretty(&codex_hooks)?)?;
    Ok(())
}

#[cfg(test)]
mod receiver_setup_tests {
    use super::{ensure_hook_entry, parse_receiver_channels, ReceiverSetupChannels};
    use serde_json::json;

    #[test]
    fn hook_merge_is_idempotent_and_preserves_other_settings() {
        let mut settings = json!({"permissions": {"allow": ["Read"]}});
        ensure_hook_entry(&mut settings, "SessionStart", "/tmp/session.py");
        ensure_hook_entry(&mut settings, "SessionStart", "/tmp/session.py");
        let hooks = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(settings["permissions"]["allow"][0], "Read");
    }

    #[test]
    fn channel_menu_selects_only_the_requested_configuration() {
        assert_eq!(parse_receiver_channels("1"), Some(ReceiverSetupChannels::Email));
        assert_eq!(parse_receiver_channels("2"), Some(ReceiverSetupChannels::Sms));
        assert_eq!(parse_receiver_channels("3"), Some(ReceiverSetupChannels::Both));
        assert_eq!(parse_receiver_channels("4"), None);
    }
}

/// Handle `brain habits`: ensure the shared brain server is up, then open its
/// `/habits` page in the browser. Best-effort open (fire-and-forget).
fn habits_command() -> Result<()> {
    let theme = crate::theme::Theme::active();
    eprintln!("{}", crate::server::lifecycle::format_ensure_plan(theme));
    logging::log("habits ensure server");
    let port = crate::server::lifecycle::ensure_running()?;
    let target = crate::server::url(port, "/habits");
    logging::log(format!("habits open {target}"));
    println!("{}", theme.info(&format!("Opening {target}")));
    open_in_browser(&target);
    Ok(())
}

/// Open a URL in the system browser via macOS `open <url>`. Fire-and-forget:
/// a spawn failure never fails the caller (the URL is already printed).
fn open_in_browser(url: &str) {
    logging::log(format!("spawn open {url}"));
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Handle `brain server {start|status|kill|run}`. `run` is the internal
/// blocking daemon loop; the rest manage the shared background server.
fn server_command(args: &crate::cli::ServerArgs) -> Result<()> {
    use crate::cli::ServerAction;
    match &args.action {
        ServerAction::Start => {
            logging::log("server start");
            crate::server::lifecycle::start()
        }
        ServerAction::Status => {
            logging::log("server status");
            crate::server::lifecycle::status()
        }
        ServerAction::Kill => {
            logging::log("server kill");
            crate::server::lifecycle::kill()
        }
        ServerAction::Run { port } => {
            logging::log(format!("server run port={port}"));
            crate::server::run(*port)
        }
    }
}

/// Handle `brain skills {sync}`. Bare `brain skills` defaults to `sync`.
fn skills_command(args: &crate::cli::SkillsArgs) -> Result<()> {
    let root = match &args.action {
        Some(crate::cli::SkillsAction::Sync { root }) => root.as_deref(),
        None => None,
    };
    logging::log(format!(
        "skills sync root={}",
        root.map_or_else(|| "(real)".to_owned(), |p| p.display().to_string())
    ));
    skills::command::run_sync(root)
}

/// Handle `brain personalize {show|get|set|edit}`. Bare `brain personalize`
/// runs first-run onboarding when nothing is set, otherwise shows current
/// values. Every mutating path re-renders the installed skills.
fn personalize_command(args: &PersonalizeArgs) -> Result<()> {
    match args.action.as_ref() {
        Some(PersonalizeAction::Show) => {
            logging::log("personalize show");
            personalization::command::run_show();
            Ok(())
        }
        Some(PersonalizeAction::Get { field }) => {
            logging::log(format!("personalize get field={field}"));
            personalization::command::run_get(field);
            Ok(())
        }
        Some(PersonalizeAction::Set { assignment }) => {
            logging::log(format!("personalize set assignment={assignment:?}"));
            if assignment.contains('=') {
                personalization::command::run_set(assignment)
            } else {
                personalize_set_interactive(assignment)
            }
        }
        Some(PersonalizeAction::Edit) => {
            logging::log("personalize edit");
            personalization::command::run_edit()
        }
        None => {
            logging::log("personalize default");
            personalization::onboarding::run_or_show()
        }
    }
}

/// Handle `brain config {list|get|set}`. Output goes to stdout; `get` on an
/// unset variable notes so on stderr. Bare `brain config` lists.
fn config_command(args: &ConfigArgs) -> Result<()> {
    match args.action.as_ref().unwrap_or(&ConfigAction::List) {
        ConfigAction::List => {
            logging::log("config list");
            print!(
                "{}",
                settings::render_list(&settings::resolve_all(), theme::Theme::active())
            );
        }
        ConfigAction::Get { name } => {
            let name = settings::normalize_name(name);
            logging::log(format!("config get name={name}"));
            match settings::resolve_one(&name) {
                Some(v) => println!("{v}"),
                None => eprintln!("{name} is unset"),
            }
        }
        ConfigAction::Set { assignment } => {
            if let Some((raw_name, value)) = assignment.split_once('=') {
                // Non-interactive: `name=value`.
                let (name, value) = (settings::normalize_name(raw_name), value.trim());
                logging::log(format!("config set name={name}"));
                settings::set(&name, value)?;
                // Any config change re-renders the installed skills so they
                // never drift from the user's values.
                skills::resync_skills();
                println!(
                    "{}",
                    settings::set_confirmation(&name, value, theme::Theme::active())
                );
            } else {
                // Interactive: bare `name` with no value.
                logging::log(format!("config set interactive name={assignment}"));
                config_set_interactive(&settings::normalize_name(assignment))?;
            }
        }
    }
    Ok(())
}

/// Handle `brain sync [--push|--pull] {setup|repair|status|conflicts|resolve}`.
fn sync_command(args: &SyncArgs) -> Result<()> {
    use crate::sync::args::Direction;
    let cfg = crate::sync::config::SyncConfig::load();
    let root = paths::brain_root()?;
    match &args.action {
        Some(SyncAction::Setup) => {
            logging::log("sync setup");
            crate::sync::setup::run()
        }
        Some(SyncAction::Repair) => {
            logging::log("sync repair");
            run_sync(&cfg, &root, Direction::Resync, args.if_idle)
        }
        Some(SyncAction::Init) => {
            let theme = crate::theme::Theme::active();
            eprintln!(
                "{}",
                theme.warning(
                    "`brain sync init` was renamed to `brain sync repair`; running repair now."
                )
            );
            logging::log("sync init alias -> repair");
            run_sync(&cfg, &root, Direction::Resync, args.if_idle)
        }
        Some(SyncAction::Status) => {
            logging::log("sync status");
            crate::sync::command::print_status(&cfg, &root)
        }
        Some(SyncAction::Conflicts { json }) => {
            logging::log(format!("sync conflicts json={json}"));
            crate::sync::command::print_conflicts(&root, *json)
        }
        Some(SyncAction::Resolve { originals }) => {
            logging::log(format!("sync resolve originals={originals:?}"));
            crate::sync::command::resolve(&root, originals)
        }
        None => {
            let dir = crate::sync::command::direction_from_flags(args.push, args.pull)?;
            logging::log(format!(
                "sync run direction={} if_idle={}",
                crate::sync::command::direction_label(dir),
                args.if_idle
            ));
            run_sync(&cfg, &root, dir, args.if_idle)
        }
    }
}

/// Shared: run one sync and print the outcome.
///
/// `if_idle` selects the busy-lock behavior: a background trigger passes `true`
/// and exits silently when a sync is already running (coalesce); a user-run
/// `brain sync` passes `false` and instead *follows* the in-flight sync.
fn run_sync(
    cfg: &crate::sync::config::SyncConfig,
    root: &std::path::Path,
    dir: crate::sync::args::Direction,
    if_idle: bool,
) -> Result<()> {
    if !cfg.is_configured() {
        logging::log("sync not configured");
        println!(
            "{}",
            crate::sync::command::format_unconfigured_sync_guidance(
                dir,
                crate::theme::Theme::active(),
            )
        );
        return Ok(());
    }
    logging::log(format!(
        "sync acquire lock {}",
        crate::sync::lock::default_path().display()
    ));
    let Some(_guard) = crate::sync::lock::try_acquire(&crate::sync::lock::default_path()) else {
        if if_idle {
            // A detached background trigger: a sync is already covering this, so
            // coalesce — exit silently rather than stacking a second run.
            logging::log("sync lock busy; if-idle coalesce");
            return Ok(());
        }
        // A user-run `brain sync`: don't start a second and don't error — attach
        // and mirror the in-flight sync's live progress until it finishes.
        logging::log("sync lock busy; following in-flight sync");
        crate::sync::follow::follow_until_done();
        return Ok(());
    };
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    logging::log(format!("sync start ts={ts}"));
    let outcome = crate::sync::command::sync_once(cfg, root, dir, (&ts, &ts, &date))?;
    logging::log(format!("sync outcome={}", outcome.label()));
    match outcome {
        crate::sync::verify::Outcome::Clean => println!("sync complete."),
        crate::sync::verify::Outcome::NeedsAttention(m)
        | crate::sync::verify::Outcome::Aborted(m) => {
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
            logging::log("env list");
            println!(
                "{}",
                settings::render_list(&env::resolve_all(), theme::Theme::active())
            );
        }
        EnvAction::Get { name } => {
            let name = settings::normalize_name(name);
            logging::log(format!("env get name={name}"));
            match env::resolve_one(&name) {
                Some(v) => println!("{v}"),
                None => eprintln!("{name} is unset"),
            }
        }
        EnvAction::Set { assignment } => {
            if let Some((name, value)) = assignment.split_once('=') {
                let name = settings::normalize_name(name);
                logging::log(format!("env set name={name}"));
                env::set(&name, value)?;
                println!(
                    "{}",
                    settings::set_confirmation(&name, value, theme::Theme::active())
                );
            } else {
                logging::log(format!("env set interactive name={assignment}"));
                env_set_interactive(&settings::normalize_name(assignment))?;
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
    println!(
        "{}",
        settings::set_confirmation(name, value, theme::Theme::active())
    );
    Ok(())
}

/// Interactive `brain env set <name>` (no `=value`): prompts once on
/// `/dev/tty`, then writes via [`env::set`] (which already validates the
/// name) and prints the same themed confirmation as the non-interactive path.
/// Mirrors [`config_set_interactive`].
fn env_set_interactive(name: &str) -> Result<()> {
    let Some(value) = prompt_tty_line(&format!("Set {name} = "))? else {
        // No terminal (headless): can't prompt. Point at the non-interactive form.
        anyhow::bail!("no terminal for interactive set; use `brain env set {name}=<value>`");
    };
    let value = value.trim();
    env::set(name, value)?;
    println!(
        "{}",
        settings::set_confirmation(name, value, theme::Theme::active())
    );
    Ok(())
}

/// Interactive `brain personalize set <field>` (no `=value`): prompts once on
/// `/dev/tty`, then delegates to [`personalization::command::run_set`] with
/// the assembled `field=value` assignment.
fn personalize_set_interactive(field: &str) -> Result<()> {
    let field = settings::normalize_name(field);
    let Some(value) = prompt_tty_line(&format!("Set {field} = "))? else {
        // No terminal (headless): can't prompt. Point at the non-interactive form.
        anyhow::bail!(
            "no terminal for interactive set; use `brain personalize set {field}=<value>`"
        );
    };
    personalization::command::run_set(&format!("{field}={}", value.trim()))
}

/// Prompt once on `/dev/tty` and read a line. `Ok(None)` when there is no
/// controlling terminal (so callers can fall back rather than hang).
fn prompt_tty_line(prompt: &str) -> Result<Option<String>> {
    use std::io::{BufRead, BufReader, Write};
    let Ok(tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    else {
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
fn tasks_launch(
    mut cli: TasksCli,
    agent_kind: session::AgentKind,
    with_receiver: bool,
) -> Result<()> {
    let today = Local::now().date_naive();
    let initial = match cli.command.take() {
        Some(TasksCommand::Complete(args)) => return tasks::complete::run(&args.id),
        Some(TasksCommand::Search(args)) => Initial::CustomSearch(args.query.join(" ")),
        Some(TasksCommand::Doctor) => {
            let db_path = state::Db::default_path();
            let settings_dir = std::env::var_os("HOME").map_or_else(
                || PathBuf::from(".claude"),
                |h| PathBuf::from(h).join("brain").join(".claude"),
            );
            eprintln!(
                "{}",
                tasks::doctor::format_doctor_plan(
                    &db_path,
                    &settings_dir.join("settings.json"),
                    theme::Theme::active(),
                )
            );
            let diag = tasks::doctor::run_doctor(&db_path, &settings_dir);
            std::process::exit(tasks::doctor::print_report(&diag));
        }
        None => resolve_query(&cli.query, today),
    };
    tasks_browse(initial, &mut cli, today, agent_kind, with_receiver)
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

fn tasks_browse(
    initial: Initial,
    cli: &mut TasksCli,
    today: NaiveDate,
    agent_kind: session::AgentKind,
    with_receiver: bool,
) -> Result<()> {
    logging::log("tasks browse");
    let csv_path = cli.csv.clone().unwrap_or_else(default_csv_path);
    logging::log(format!("tasks csv {}", csv_path.display()));
    let all_tasks = tasks::task::load_tasks(&csv_path)?;
    logging::log(format!("loaded {} tasks", all_tasks.len()));
    let habits_path = csv_path.with_file_name("habits.csv");
    logging::log(format!("habits csv {}", habits_path.display()));
    let habits = tasks::task::load_habits(&habits_path).unwrap_or_default();
    logging::log(format!("loaded {} habits", habits.len()));

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
    logging::log(format!(
        "build tasks view start_view={:?} initial_rows={} no_tui={}",
        start_view,
        initial_data.len(),
        cli.display.no_tui,
    ));
    let view = tasks::view::build_view(cli, &selector, start_view, initial_data, today);
    logging::log(format!(
        "built tasks view title={:?} shown={} total={}",
        view.title,
        view.tasks.len(),
        view.total,
    ));

    if cli.display.no_tui {
        logging::log("render tasks no-tui");
        tasks::plain::print_plain(&view, today, cli.display.full_notes);
    } else {
        logging::set_stdout_enabled(false);
        logging::log("enter tui");
        tui::run_tui(
            &view,
            cli,
            agent_kind,
            today,
            csv_path,
            all_tasks,
            habits,
            start_view,
            initial_search,
            with_receiver,
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

/// Remove a trailing-delegated Codex flag and report whether it was present.
/// Top-level clap handles `brain --codex` / `brain -cx`; this catches
/// `brain tasks --codex` / `brain tasks -cx`, because `tasks` intentionally
/// captures hyphenated trailing args verbatim.
fn take_codex_flag(args: &mut Vec<String>) -> bool {
    let before = args.len();
    args.retain(|arg| arg != "--codex" && arg != "-cx");
    args.len() != before
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_codex_flag_removes_tasks_trailing_flag() {
        let mut args = vec!["today".to_owned(), "--codex".to_owned(), "--mit".to_owned()];
        assert!(take_codex_flag(&mut args));
        assert_eq!(args, vec!["today".to_owned(), "--mit".to_owned()]);
    }

    #[test]
    fn take_codex_flag_removes_tasks_trailing_cx_alias() {
        let mut args = vec!["today".to_owned(), "-cx".to_owned(), "--mit".to_owned()];
        assert!(take_codex_flag(&mut args));
        assert_eq!(args, vec!["today".to_owned(), "--mit".to_owned()]);
    }

    #[test]
    fn take_codex_flag_leaves_args_when_absent() {
        let mut args = vec!["today".to_owned()];
        assert!(!take_codex_flag(&mut args));
        assert_eq!(args, vec!["today".to_owned()]);
    }
}
