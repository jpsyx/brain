//! Receiver setup and lifecycle command dispatch.

use anyhow::Result;

mod details;
mod enablement;
mod hooks;
mod identity;
mod setup;
mod url;

pub(crate) use enablement::{
    ReceiverIntentRefresher, apply_receiver_action_with, apply_startup_receiver_flag,
    read_receiver_status, receiver_enabled,
};
use enablement::{print_receiver_change, print_receiver_status};

/// Refresh the bundled lifecycle hooks before an agent-capable TUI starts.
pub(crate) fn refresh_agent_hooks(root: &std::path::Path) -> Result<()> {
    hooks::install(root)
}

pub fn run_receiver(
    args: &crate::cli::ReceiverArgs,
    context: &crate::workspace::CommandContext,
    explicit_workspace: bool,
) -> Result<()> {
    run_receiver_with_refresher(
        args,
        context,
        explicit_workspace,
        &crate::server::control::ServerClient::default(),
    )
}

pub(crate) fn run_receiver_with_refresher(
    args: &crate::cli::ReceiverArgs,
    context: &crate::workspace::CommandContext,
    explicit_workspace: bool,
    refresher: &dyn ReceiverIntentRefresher,
) -> Result<()> {
    use crate::cli::ReceiverServerAction;
    let Some(action) = &args.action else {
        return details::run(context, explicit_workspace);
    };
    match action {
        ReceiverServerAction::Setup(args) => {
            run_configuration_command(context.workspace.id(), refresher, || {
                setup::run(args, context)
            })
        }
        ReceiverServerAction::Set { assignment } => {
            run_configuration_command(context.workspace.id(), refresher, || {
                receiver_set(context, assignment.as_deref())
            })
        }
        ReceiverServerAction::Start => {
            let outcome = apply_receiver_action_with(
                context,
                crate::workspace::ReceiverAction::Start,
                refresher,
            )?;
            print_receiver_change(&outcome);
            Ok(())
        }
        ReceiverServerAction::Stop => {
            let outcome = apply_receiver_action_with(
                context,
                crate::workspace::ReceiverAction::Stop,
                refresher,
            )?;
            print_receiver_change(&outcome);
            Ok(())
        }
        ReceiverServerAction::Status => print_receiver_status(context),
        ReceiverServerAction::Url(args) => url::run(args, context),
        ReceiverServerAction::Email => {
            identity::run(context, crate::server::receiver::Channel::Email)
        }
        ReceiverServerAction::Phone => {
            identity::run(context, crate::server::receiver::Channel::Sms)
        }
        ReceiverServerAction::Logs => crate::server::lifecycle::logs(),
    }
}

fn run_configuration_command(
    workspace_id: crate::workspace::WorkspaceId,
    refresher: &dyn ReceiverIntentRefresher,
    operation: impl FnOnce() -> Result<()>,
) -> Result<()> {
    operation()?;
    if let Err(error) = refresher.refresh_enabled(workspace_id) {
        eprintln!(
            "{}",
            crate::theme::Theme::active().warning(&format!(
                "Warning: receiver configuration was saved, but the live shared server could not reload it: {error:#}"
            ))
        );
    }
    Ok(())
}

fn receiver_env_fields() -> Vec<(&'static str, &'static str, &'static str, bool)> {
    [
        "brain_receiver_public_url",
        "twilio_account_sid",
        "twilio_auth_token",
        "twilio_from_number",
        "resend_api_key",
        "resend_from_email",
        "resend_webhook_signing_secret",
    ]
    .into_iter()
    .map(|name| {
        let (label, description, secret) = setup::provider_prompt(name);
        (name, label, description, secret)
    })
    .collect()
}

fn receiver_set(
    context: &crate::workspace::CommandContext,
    assignment: Option<&str>,
) -> Result<()> {
    let fields = receiver_env_fields();
    let name = if let Some(assignment) = assignment {
        assignment
            .split_once('=')
            .map_or_else(|| assignment.to_owned(), |(name, _)| name.to_owned())
    } else {
        println!(
            "{}",
            crate::theme::Theme::active().heading("Receiver environment")
        );
        for (index, (_, label, description, _)) in fields.iter().enumerate() {
            println!(
                "  {}) {}",
                index + 1,
                crate::theme::Theme::active().accent(label)
            );
            println!("     {}", crate::theme::Theme::active().muted(description));
        }
        let Some(choice) =
            crate::command::configuration::prompt_tty_line("Choose a variable number: ")?
        else {
            anyhow::bail!("receiver set needs an interactive terminal");
        };
        let index = choice
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|index| (1..=fields.len()).contains(index))
            .ok_or_else(|| anyhow::anyhow!("choose a number from 1 to {}", fields.len()))?;
        fields[index - 1].0.to_owned()
    };
    let Some((_, label, _, secret)) = fields.iter().find(|(field, ..)| *field == name) else {
        anyhow::bail!("unknown receiver environment variable: {name}");
    };
    if let Some((_, value)) = assignment.and_then(|value| value.split_once('=')) {
        return crate::env::set(context, &name, value);
    }
    prompt_receiver_value(context, &name, label, *secret)
}

fn prompt_receiver_value(
    context: &crate::workspace::CommandContext,
    name: &str,
    label: &str,
    secret: bool,
) -> Result<()> {
    let old = crate::env::get(context, name).unwrap_or_default();
    let hint = if old.trim().is_empty() {
        "(not set)"
    } else {
        "(saved)"
    };
    let prompt = format!("{} {hint}: ", crate::theme::Theme::active().prompt(label));
    let input = if secret {
        crate::command::configuration::prompt_masked_line(&prompt)?
    } else {
        crate::command::configuration::prompt_tty_line(&prompt)?
    }
    .ok_or_else(|| anyhow::anyhow!("receiver set needs an interactive terminal"))?;
    let value = match input.trim() {
        "" => old,
        "/clear" => String::new(),
        value => value.to_owned(),
    };
    crate::env::set(context, name, &value)
}

#[cfg(test)]
mod tests;
