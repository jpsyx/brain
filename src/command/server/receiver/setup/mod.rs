//! Workspace-specific receiver provider and portable-user setup.

mod user;

use anyhow::{Context as _, Result};

use crate::cli::{ReceiverSetupArgs, ReceiverSetupChannels};
use crate::server::receiver::Channel;
use crate::workspace::CommandContext;

struct SetupPlan {
    channels: ReceiverSetupChannels,
    providers: Vec<(&'static str, String)>,
    users: crate::users::Users,
}

pub(super) fn run(args: &ReceiverSetupArgs, context: &CommandContext) -> Result<()> {
    let plan = if let Some(channels) = args.channels {
        SetupPlan {
            channels,
            providers: provider_values(args, context, channels)?,
            users: user::headless_plan(args, &context.workspace, channels)?,
        }
    } else {
        interactive_plan(context)?
    };
    crate::env::set_many(context, &plan.providers)?;
    crate::users::UsersStore::save(&context.workspace, &plan.users)?;
    super::hooks::install(context.workspace.root())?;
    print_urls(context, plan.channels)?;
    println!(
        "{}",
        crate::theme::Theme::active().success("receiver configuration saved")
    );
    Ok(())
}

fn interactive_plan(context: &CommandContext) -> Result<SetupPlan> {
    let theme = crate::theme::Theme::active();
    println!("{}", theme.heading("Set up the brain receiver"));
    println!("{}", theme.muted("Choose which channels to configure:"));
    println!("  {}", theme.accent("1) Email"));
    println!("  {}", theme.accent("2) SMS"));
    println!("  {}", theme.accent("3) Both"));
    let choice = prompt_line(&format!("{} ", theme.prompt("Choose 1, 2, or 3:")))?;
    let channels = parse_channels(&choice)?;
    println!(
        "{}",
        theme.muted("Press Enter to keep an existing value. Type /clear to erase it.")
    );
    let providers = provider_fields(channels)
        .into_iter()
        .map(|name| {
            let (label, description, secret) = provider_prompt(name);
            println!("{}", theme.muted(description));
            prompt_provider_value(context, name, label, secret).map(|value| (name, value))
        })
        .collect::<Result<Vec<_>>>()?;
    let users = user::interactive_plan(&context.workspace, channels)?;
    Ok(SetupPlan {
        channels,
        providers,
        users,
    })
}

fn provider_values(
    args: &ReceiverSetupArgs,
    context: &CommandContext,
    channels: ReceiverSetupChannels,
) -> Result<Vec<(&'static str, String)>> {
    provider_fields(channels)
        .into_iter()
        .map(|name| {
            let supplied = provider_arg(args, name);
            let current = crate::env::get(context, name);
            let value = supplied
                .or(current)
                .filter(|value| !value.trim().is_empty());
            value
                .map(|value| (name, value))
                .with_context(|| format!("receiver setup requires --{}", provider_cli_flag(name)))
        })
        .collect()
}

fn provider_cli_flag(name: &str) -> String {
    match name {
        "brain_receiver_public_url" => "public-url".to_owned(),
        _ => name.replace('_', "-"),
    }
}

fn provider_arg(args: &ReceiverSetupArgs, name: &str) -> Option<String> {
    match name {
        "brain_receiver_public_url" => args.public_url.clone(),
        "twilio_account_sid" => args.twilio_account_sid.clone(),
        "twilio_auth_token" => args.twilio_auth_token.clone(),
        "twilio_from_number" => args.twilio_from_number.clone(),
        "resend_api_key" => args.resend_api_key.clone(),
        "resend_from_email" => args.resend_from_email.clone(),
        "resend_webhook_signing_secret" => args.resend_webhook_signing_secret.clone(),
        _ => None,
    }
}

pub(super) fn provider_fields(channels: ReceiverSetupChannels) -> Vec<&'static str> {
    let mut fields = vec!["brain_receiver_public_url"];
    if sms(channels) {
        fields.extend([
            "twilio_account_sid",
            "twilio_auth_token",
            "twilio_from_number",
        ]);
    }
    if email(channels) {
        fields.extend([
            "resend_api_key",
            "resend_from_email",
            "resend_webhook_signing_secret",
        ]);
    }
    fields
}

pub(super) fn provider_prompt(name: &str) -> (&'static str, &'static str, bool) {
    match name {
        "brain_receiver_public_url" => (
            "Public base URL",
            "Enter the public base URL. Brain derives workspace-specific webhook paths from it.",
            false,
        ),
        "twilio_account_sid" => (
            "Twilio Account SID",
            "Your Twilio Account SID for SMS delivery and media downloads.",
            false,
        ),
        "twilio_auth_token" => (
            "Twilio Auth Token",
            "Your Twilio Auth Token. Input is hidden and stored only in machine-local env.",
            true,
        ),
        "twilio_from_number" => (
            "Twilio From number",
            "The Twilio phone number Brain uses for outbound SMS, including country code.",
            false,
        ),
        "resend_api_key" => (
            "Resend API key",
            "Your Resend API key for receiving and sending email. Input is hidden.",
            true,
        ),
        "resend_from_email" => (
            "Resend From email",
            "The verified Resend sender address for outbound email.",
            false,
        ),
        "resend_webhook_signing_secret" => (
            "Resend webhook signing secret",
            "The Resend/Svix webhook signing secret. Input is hidden.",
            true,
        ),
        _ => unreachable!("unknown receiver provider field: {name}"),
    }
}

fn prompt_provider_value(
    context: &CommandContext,
    name: &str,
    label: &str,
    secret: bool,
) -> Result<String> {
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
    .ok_or_else(|| anyhow::anyhow!("receiver setup needs an interactive terminal"))?;
    Ok(match input.trim() {
        "" => old,
        "/clear" => String::new(),
        value => value.to_owned(),
    })
}

fn prompt_line(prompt: &str) -> Result<String> {
    crate::command::configuration::prompt_tty_line(prompt)?
        .ok_or_else(|| anyhow::anyhow!("receiver setup needs an interactive terminal"))
}

fn parse_channels(input: &str) -> Result<ReceiverSetupChannels> {
    match input.trim() {
        "1" => Ok(ReceiverSetupChannels::Email),
        "2" => Ok(ReceiverSetupChannels::Sms),
        "3" => Ok(ReceiverSetupChannels::Both),
        _ => anyhow::bail!("choose 1 for email, 2 for SMS, or 3 for both"),
    }
}

fn print_urls(context: &CommandContext, channels: ReceiverSetupChannels) -> Result<()> {
    let manifest = crate::workspace::WorkspaceManifest::load(
        context.workspace.root(),
        env!("CARGO_PKG_VERSION"),
    )?;
    anyhow::ensure!(
        manifest.workspace_id() == context.workspace.id(),
        "workspace manifest UUID changed during receiver setup"
    );
    let ingress = crate::server::IngressId::from(manifest.receiver_ingress_id());
    let public_url = crate::env::get(context, "brain_receiver_public_url").unwrap_or_default();
    let theme = crate::theme::Theme::active();
    if sms(channels) {
        println!(
            "{}",
            theme.muted(&format!(
                "Twilio webhook URL: {}",
                crate::server::receiver::http::receiver_webhook_url(
                    &public_url,
                    ingress,
                    Channel::Sms,
                )
            ))
        );
    }
    if email(channels) {
        println!(
            "{}",
            theme.muted(&format!(
                "Resend webhook URL: {}",
                crate::server::receiver::http::receiver_webhook_url(
                    &public_url,
                    ingress,
                    Channel::Email,
                )
            ))
        );
    }
    Ok(())
}

pub(super) const fn sms(channels: ReceiverSetupChannels) -> bool {
    matches!(
        channels,
        ReceiverSetupChannels::Sms | ReceiverSetupChannels::Both
    )
}

pub(super) const fn email(channels: ReceiverSetupChannels) -> bool {
    matches!(
        channels,
        ReceiverSetupChannels::Email | ReceiverSetupChannels::Both
    )
}

#[cfg(test)]
mod tests;
