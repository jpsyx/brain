//! Config, environment, personalization, and skill command handlers.

use anyhow::{Result, anyhow};

use crate::cli::{
    ConfigAction, ConfigArgs, EnvAction, EnvArgs, PersonalizeAction, PersonalizeArgs,
};

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
            println!(
                "{}",
                crate::settings::render_list(
                    &crate::env::resolve_all(context),
                    crate::theme::Theme::active(),
                )
            );
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
                    env_set_confirmation(&name, value, crate::theme::Theme::active())
                );
            } else {
                crate::logging::log("env set interactive");
                env_set_interactive(context, assignment.as_deref())?;
            }
        }
    }
    Ok(())
}

pub fn run_personalize(
    args: &PersonalizeArgs,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    match args.action.as_ref() {
        Some(PersonalizeAction::Show) => {
            crate::logging::log("personalize show");
            crate::personalization::command::run_show(&context.workspace);
            Ok(())
        }
        Some(PersonalizeAction::Get { field }) => {
            crate::logging::log(format!("personalize get field={field}"));
            crate::personalization::command::run_get(&context.workspace, field);
            Ok(())
        }
        Some(PersonalizeAction::Set { assignment }) => {
            crate::logging::log(format!("personalize set assignment={assignment:?}"));
            if assignment.contains('=') {
                crate::personalization::command::run_set(&context.workspace, assignment)
            } else {
                personalize_set_interactive(&context.workspace, assignment)
            }
        }
        Some(PersonalizeAction::Edit) => {
            crate::logging::log("personalize edit");
            crate::personalization::command::run_edit(&context.workspace)
        }
        None => {
            crate::logging::log("personalize default");
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
            "no terminal for interactive set; use `brain config set {name}=<value>`"
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
        crate::settings::normalize_name(name)
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
    let Some(value) = (if crate::env::is_sensitive(&name) {
        prompt_masked_line(&format!("Set {name} = "))?
    } else {
        prompt_tty_line(&format!("Set {name} = "))?
    }) else {
        anyhow::bail!("no terminal for interactive set; use `brain env set {name}=<value>`");
    };
    let value = value.trim();
    crate::env::set(context, &name, value)?;
    println!(
        "{}",
        env_set_confirmation(&name, value, crate::theme::Theme::active())
    );
    Ok(())
}

fn env_set_confirmation(name: &str, value: &str, theme: crate::theme::Theme) -> String {
    if crate::env::is_sensitive(name) {
        format!("{} saved", theme.accent(name))
    } else {
        crate::settings::set_confirmation(name, value, theme)
    }
}

fn personalize_set_interactive(
    workspace: &crate::workspace::WorkspaceContext,
    field: &str,
) -> Result<()> {
    let field = crate::settings::normalize_name(field);
    let Some(value) = prompt_tty_line(&format!("Set {field} = "))? else {
        anyhow::bail!(
            "no terminal for interactive set; use `brain personalize set {field}=<value>`"
        );
    };
    crate::personalization::command::run_set(workspace, &format!("{field}={}", value.trim()))
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
