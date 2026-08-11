//! Workspace-specific receiver provider and portable-user setup.

mod transaction;
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
    let mut plan = if let Some(channels) = args.channels.filter(|_| args.user_id.is_some()) {
        SetupPlan {
            channels,
            providers: provider_values(args, context, channels)?,
            users: user::headless_plan(args, &context.workspace, channels)?,
        }
    } else {
        interactive_plan(context, args.channels)?
    };
    validate_plan(&mut plan)?;
    verify_selected_identity(context)?;
    transaction::persist_plan(&plan, context)?;
    print_urls(&plan);
    println!(
        "{}",
        crate::theme::Theme::active().success("receiver configuration saved")
    );
    Ok(())
}

#[cfg(test)]
fn uses_headless_setup(args: &ReceiverSetupArgs) -> bool {
    args.channels.is_some() && args.user_id.is_some()
}

fn interactive_plan(
    context: &CommandContext,
    selected_channels: Option<ReceiverSetupChannels>,
) -> Result<SetupPlan> {
    let theme = crate::theme::Theme::active();
    println!("{}", theme.heading("Set up the brain receiver"));
    let channels = if let Some(channels) = selected_channels {
        let channel_name = format!("{channels:?}").to_ascii_lowercase();
        println!(
            "{} {}",
            theme.muted("Configuring selected channels:"),
            theme.accent(&channel_name)
        );
        channels
    } else {
        println!("{}", theme.muted("Choose which channels to configure:"));
        println!("  {}", theme.accent("1) Email"));
        println!("  {}", theme.accent("2) SMS"));
        println!("  {}", theme.accent("3) Both"));
        let choice = prompt_line(&format!("{} ", theme.prompt("Choose 1, 2, or 3:")))?;
        parse_channels(&choice)?
    };
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
            Ok((name, supplied.or(current).unwrap_or_default()))
        })
        .collect()
}

fn validate_plan(plan: &mut SetupPlan) -> Result<()> {
    for name in provider_fields(plan.channels) {
        let value = provider_value_mut(&mut plan.providers, name)
            .with_context(|| format!("receiver setup requires --{}", provider_cli_flag(name)))?;
        anyhow::ensure!(
            !value.trim().is_empty(),
            "receiver setup requires --{}",
            provider_cli_flag(name)
        );
        match name {
            "brain_receiver_public_url" => *value = validate_public_base_url(value)?,
            "twilio_from_number" => {
                *value = crate::users::normalize_phone(value)
                    .map_err(|_| anyhow::anyhow!("Twilio sender phone number is invalid"))?;
            }
            "resend_from_email" => {
                *value = crate::users::normalize_email(value)
                    .map_err(|_| anyhow::anyhow!("Resend sender email address is invalid"))?;
            }
            _ => anyhow::ensure!(
                !value.chars().any(char::is_control),
                "receiver provider value is invalid"
            ),
        }
    }
    Ok(())
}

fn provider_value<'a>(providers: &'a [(&str, String)], name: &str) -> Option<&'a str> {
    providers
        .iter()
        .find_map(|(candidate, value)| (*candidate == name).then_some(value.as_str()))
}

fn provider_value_mut<'a>(
    providers: &'a mut [(&str, String)],
    name: &str,
) -> Option<&'a mut String> {
    providers
        .iter_mut()
        .find_map(|(candidate, value)| (*candidate == name).then_some(value))
}

fn validate_public_base_url(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_end_matches('/');
    let authority = trimmed
        .strip_prefix("https://")
        .ok_or_else(|| anyhow::anyhow!("receiver public URL must use HTTPS"))?;
    anyhow::ensure!(
        !authority.is_empty()
            && !authority.contains(['/', '?', '#', '@'])
            && !authority.chars().any(char::is_whitespace)
            && !authority.chars().any(char::is_control),
        "receiver public URL must be an HTTPS origin without a path, query, or fragment"
    );
    validate_authority(authority)?;
    Ok(trimmed.to_owned())
}

fn validate_authority(authority: &str) -> Result<()> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            anyhow::bail!("receiver public URL host is invalid");
        };
        anyhow::ensure!(
            host.parse::<std::net::Ipv6Addr>().is_ok(),
            "receiver public URL host is invalid"
        );
        return validate_port_suffix(suffix);
    }
    let (host, port) = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    anyhow::ensure!(
        !host.is_empty()
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
            && !host.starts_with(['.', '-'])
            && !host.ends_with(['.', '-']),
        "receiver public URL host is invalid"
    );
    if let Some(port) = port {
        validate_port(port)?;
    }
    Ok(())
}

fn validate_port_suffix(suffix: &str) -> Result<()> {
    if suffix.is_empty() {
        return Ok(());
    }
    let port = suffix
        .strip_prefix(':')
        .ok_or_else(|| anyhow::anyhow!("receiver public URL host is invalid"))?;
    validate_port(port)
}

fn validate_port(port: &str) -> Result<()> {
    anyhow::ensure!(
        port.parse::<u16>().is_ok_and(|port| port > 0),
        "receiver public URL port is invalid"
    );
    Ok(())
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
    Ok(resolve_provider_input(&old, &input))
}

fn resolve_provider_input(old: &str, input: &str) -> String {
    match input.trim() {
        "" => old.to_owned(),
        "/clear" => String::new(),
        value => value.to_owned(),
    }
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

/// Refuse to write provider credentials into a workspace whose portable
/// identity no longer matches the one that was selected.
///
/// The webhook URLs no longer name a workspace, but the credentials this setup
/// persists still belong to exactly one, so the identity check stays.
fn verify_selected_identity(context: &CommandContext) -> Result<()> {
    let manifest = crate::workspace::WorkspaceManifest::load(
        context.workspace.root(),
        env!("CARGO_PKG_VERSION"),
    )?;
    anyhow::ensure!(
        manifest.workspace_id() == context.workspace.id(),
        "workspace manifest UUID changed during receiver setup"
    );
    Ok(())
}

fn print_urls(plan: &SetupPlan) {
    let public_url = provider_value(&plan.providers, "brain_receiver_public_url")
        .expect("validated setup includes public URL");
    let theme = crate::theme::Theme::active();
    if sms(plan.channels) {
        println!(
            "{}",
            theme.muted(&format!(
                "Twilio webhook URL: {}",
                crate::server::receiver::http::receiver_webhook_url(public_url, Channel::Sms)
            ))
        );
    }
    if email(plan.channels) {
        println!(
            "{}",
            theme.muted(&format!(
                "Resend webhook URL: {}",
                crate::server::receiver::http::receiver_webhook_url(public_url, Channel::Email)
            ))
        );
    }
    // The URL names no workspace, so the number and address this setup just
    // saved are what will route a message to it.
    println!(
        "{}",
        theme.muted(
            "Both URLs are machine-wide: brain routes each message by the number or address it arrived at."
        )
    );
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
