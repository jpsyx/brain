//! Config, environment, personalization, and skill command handlers.

use anyhow::{Result, anyhow};

use crate::cli::{ConfigAction, ConfigArgs, EnvAction, EnvArgs, PersonaAction, PersonaArgs};

pub fn run_config(args: &ConfigArgs, context: &crate::workspace::CommandContext) -> Result<()> {
    match args.action.as_ref().unwrap_or(&ConfigAction::List) {
        ConfigAction::List => {
            crate::logging::log("config list");
            print!(
                "{}",
                crate::settings::render_list(
                    &crate::settings::resolve_all(&context.workspace),
                    crate::theme::Theme::active(),
                )
            );
        }
        ConfigAction::Get { name } => {
            let name = crate::settings::normalize_name(name);
            crate::logging::log(format!("config get name={name}"));
            match crate::settings::resolve_one(&context.workspace, &name) {
                Some(value) => println!("{value}"),
                None => eprintln!("{name} is unset"),
            }
        }
        ConfigAction::Set { assignment } => {
            if let Some((raw_name, value)) = assignment.split_once('=') {
                let (name, value) = (crate::settings::normalize_name(raw_name), value.trim());
                crate::logging::log(format!("config set name={name}"));
                crate::settings::set(&context.workspace, &name, value)?;
                crate::skills::resync_skills(&context.workspace);
                println!(
                    "{}",
                    crate::settings::set_confirmation(&name, value, crate::theme::Theme::active(),)
                );
            } else {
                crate::logging::log(format!("config set interactive name={assignment}"));
                config_set_interactive(context, &crate::settings::normalize_name(assignment))?;
            }
        }
    }
    Ok(())
}

pub fn run_env(args: &EnvArgs, context: &crate::workspace::CommandContext) -> Result<()> {
    match args.action.as_ref().unwrap_or(&EnvAction::List) {
        EnvAction::List => {
            crate::logging::log("env list");
            print!("{}", crate::env::render_breakdown(context));
        }
        EnvAction::Get { name } => {
            let name = crate::settings::normalize_name(name);
            crate::logging::log(format!("env get name={name}"));
            match crate::env::resolve_one(context, &name) {
                Some(value) => println!("{value}"),
                None => eprintln!("{name} is unset"),
            }
        }
        EnvAction::Set { assignment } => {
            if let Some(assignment) = assignment.as_deref()
                && let Some((name, value)) = assignment.split_once('=')
            {
                let name = crate::settings::normalize_name(name);
                crate::logging::log(format!("env set name={name}"));
                crate::env::set(context, &name, value)?;
                println!(
                    "{}",
                    env_set_confirmation(
                        &name,
                        &stored_value(context, &name, value),
                        crate::theme::Theme::active()
                    )
                );
            } else {
                crate::logging::log("env set interactive");
                env_set_interactive(context, assignment.as_deref())?;
            }
        }
    }
    Ok(())
}

pub fn run_persona(args: &PersonaArgs, context: &crate::workspace::CommandContext) -> Result<()> {
    match args.action.as_ref() {
        Some(PersonaAction::Show { user }) => {
            crate::logging::log(format!("persona show user={user:?}"));
            crate::personalization::command::run_show(&context.workspace, user.as_deref())
        }
        Some(PersonaAction::List) => {
            crate::logging::log("persona list");
            crate::personalization::command::run_list(&context.workspace);
            Ok(())
        }
        Some(PersonaAction::Get { user, field }) => {
            crate::logging::log(format!("persona get user={user} field={field:?}"));
            crate::personalization::command::run_get(&context.workspace, user, field.as_deref())
        }
        Some(PersonaAction::Set { assignment, user }) => {
            crate::logging::log(format!(
                "persona set assignment={assignment:?} user={user:?}"
            ));
            if assignment.contains('=') {
                crate::personalization::command::run_set(
                    &context.workspace,
                    user.as_deref(),
                    assignment,
                )
            } else {
                persona_set_interactive(&context.workspace, user.as_deref(), assignment)
            }
        }
        Some(PersonaAction::Edit) => {
            crate::logging::log("persona edit");
            crate::personalization::command::run_edit(&context.workspace)
        }
        None => {
            crate::logging::log("persona default");
            crate::personalization::onboarding::run_or_show(&context.workspace)
        }
    }
}

pub fn run_skills(
    args: &crate::cli::SkillsArgs,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    match &args.action {
        Some(crate::cli::SkillsAction::Status) => {
            crate::logging::log("skills status");
            crate::skills::command::run_status(context)
        }
        Some(crate::cli::SkillsAction::Sync { root }) => {
            crate::logging::log(format!(
                "skills sync root={}",
                root.as_deref()
                    .map_or_else(|| "(real)".to_owned(), |path| path.display().to_string())
            ));
            crate::skills::command::run_sync(&context.workspace, root.as_deref())
        }
        None => {
            crate::logging::log("skills sync root=(real)");
            crate::skills::command::run_sync(&context.workspace, None)
        }
    }
}

fn config_set_interactive(context: &crate::workspace::CommandContext, name: &str) -> Result<()> {
    match name {
        "namespaces" => {
            return crate::personalization::command::run_set_namespaces(&context.workspace);
        }
        "tags" | "tag_styles" => {
            return crate::personalization::command::run_set_tags(&context.workspace);
        }
        _ => {}
    }
    let Some(value) = prompt_tty_line(&format!("Set {name} = "))? else {
        return Err(anyhow!(
            "no terminal for interactive set; use `{}`",
            crate::workspace::suggest(&format!("config set {name}=<value>"))
        ));
    };
    let value = value.trim();
    crate::settings::set(&context.workspace, name, value)?;
    crate::skills::resync_skills(&context.workspace);
    println!(
        "{}",
        crate::settings::set_confirmation(name, value, crate::theme::Theme::active())
    );
    Ok(())
}

fn env_set_interactive(
    context: &crate::workspace::CommandContext,
    requested: Option<&str>,
) -> Result<()> {
    let name = if let Some(name) = requested {
        let name = crate::settings::normalize_name(name);
        // A JSON array is not something to type at a `Set x = ` prompt, so this
        // variable has its own add/edit/delete walkthrough. Every field stays
        // settable with a plain `brain env set skill_sessions '[…]'`.
        if name == crate::skill_session::ENV_VAR {
            return crate::skill_session::editor::run(context);
        }
        name
    } else {
        let rows = crate::env::resolve_all(context);
        println!(
            "{}",
            crate::theme::Theme::active().heading("Brain environment")
        );
        for (index, row) in rows.iter().enumerate() {
            println!(
                "  {}) {}",
                index + 1,
                crate::theme::Theme::active().accent(&row.name)
            );
            println!(
                "     {}",
                crate::theme::Theme::active().muted(&row.description)
            );
        }
        let Some(choice) = prompt_tty_line("Choose a variable number: ")? else {
            anyhow::bail!("brain env set needs an interactive terminal");
        };
        let index = choice
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|index| (1..=rows.len()).contains(index))
            .ok_or_else(|| anyhow!("choose a number from 1 to {}", rows.len()))?;
        rows[index - 1].name.clone()
    };
    if name == crate::skill_session::ENV_VAR {
        return crate::skill_session::editor::run(context);
    }
    let prompt = env_value_prompt(&name);
    let Some(value) = (if crate::env::is_sensitive(&name) {
        prompt_masked_line(&prompt)?
    } else {
        prompt_tty_line(&prompt)?
    }) else {
        anyhow::bail!("no terminal for interactive set; use `brain env set {name}=<value>`");
    };
    let value = value.trim();
    crate::env::set(context, &name, value)?;
    println!(
        "{}",
        env_set_confirmation(
            &name,
            &stored_value(context, &name, value),
            crate::theme::Theme::active()
        )
    );
    Ok(())
}

/// What the store actually holds for `name` after a write.
///
/// Enum-valued variables are canonicalized on the way in (`Open-Code` →
/// `opencode`), so echoing the typed text back would confirm something that is
/// not what a later read returns.
fn stored_value(context: &crate::workspace::CommandContext, name: &str, written: &str) -> String {
    crate::env::get(context, name).unwrap_or_else(|| written.to_owned())
}

/// The interactive prompt for one env variable, naming the accepted values when
/// the variable is an enum. Pure.
fn env_value_prompt(name: &str) -> String {
    if name == crate::agent::default_frontend::ENV_VAR {
        let values = crate::session::AgentKind::ALL
            .map(crate::session::AgentKind::as_str)
            .join(" | ");
        return format!("Set {name} ({values}) = ");
    }
    format!("Set {name} = ")
}

fn env_set_confirmation(name: &str, value: &str, theme: crate::theme::Theme) -> String {
    if crate::env::is_sensitive(name) {
        format!("{} saved", theme.accent(name))
    } else {
        crate::settings::set_confirmation(name, value, theme)
    }
}

fn persona_set_interactive(
    workspace: &crate::workspace::WorkspaceContext,
    user: Option<&str>,
    field: &str,
) -> Result<()> {
    let field = crate::settings::normalize_name(field);
    let Some(value) = prompt_tty_line(&format!("Set {field} = "))? else {
        anyhow::bail!(
            "no terminal for interactive set; use `{}`",
            crate::workspace::suggest(&format!("persona set {field}=<value>"))
        );
    };
    crate::personalization::command::run_set(workspace, user, &format!("{field}={}", value.trim()))
}

pub(crate) fn prompt_tty_line(prompt: &str) -> Result<Option<String>> {
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
        return Ok(None);
    }
    Ok(Some(line))
}

pub(crate) fn prompt_masked_line(prompt: &str) -> Result<Option<String>> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use std::io::Write;
    let Ok(tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    else {
        return Ok(None);
    };
    let mut out = tty.try_clone()?;
    write!(out, "{prompt}")?;
    out.flush()?;
    enable_raw_mode()?;
    let result = (|| -> Result<Option<String>> {
        let mut value = String::new();
        loop {
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Char(ch) => {
                    value.push(ch);
                    write!(out, "{}", masked_echo(&ch.to_string()))?;
                    out.flush()?;
                }
                KeyCode::Backspace if value.pop().is_some() => {
                    write!(out, "\x08 \x08")?;
                    out.flush()?;
                }
                KeyCode::Enter => {
                    writeln!(out)?;
                    return Ok(Some(value));
                }
                KeyCode::Esc => return Ok(None),
                _ => {}
            }
        }
    })();
    disable_raw_mode()?;
    result
}

#[must_use]
pub(crate) fn masked_echo(value: &str) -> String {
    "*".repeat(value.chars().count())
}

#[cfg(test)]
mod tests {
    use super::{env_set_confirmation, masked_echo};

    #[test]
    fn masked_echo_uses_one_star_per_character() {
        assert_eq!(masked_echo("abc"), "***");
        assert_eq!(masked_echo("éx"), "**");
        assert_eq!(masked_echo(""), "");
    }

    #[test]
    fn an_enum_env_prompt_lists_the_values_it_accepts() {
        // Human-friendly fallback: never make someone read `--help` to learn
        // what an enum variable takes.
        assert_eq!(
            super::env_value_prompt("default_agent_frontend"),
            "Set default_agent_frontend (claude | codex | opencode) = "
        );
        assert_eq!(super::env_value_prompt("claude_cmd"), "Set claude_cmd = ");
    }

    #[test]
    fn sensitive_env_confirmation_never_contains_the_assigned_value() {
        let secret = "whsec_private-value";
        let confirmation = env_set_confirmation(
            "resend_webhook_signing_secret",
            secret,
            crate::theme::Theme::dark(false),
        );

        assert!(!confirmation.contains(secret));
        assert!(confirmation.contains("saved"));
    }
}
