//! Receiver setup and lifecycle command dispatch.

use anyhow::Result;

pub fn run_receiver(
    args: &crate::cli::ReceiverArgs,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    use crate::cli::ReceiverServerAction;
    match &args.action {
        ReceiverServerAction::Setup => receiver_setup(context.workspace.root()),
        ReceiverServerAction::Set { assignment } => receiver_set(assignment.as_deref()),
        action => {
            let command = match action {
                ReceiverServerAction::Start => "start",
                ReceiverServerAction::Stop => "stop",
                ReceiverServerAction::Restart => "restart",
                ReceiverServerAction::Status => "status",
                ReceiverServerAction::Logs => "logs",
                ReceiverServerAction::Setup | ReceiverServerAction::Set { .. } => unreachable!(),
            };
            match crate::server::receiver::send_control(command) {
                Ok(response) => {
                    print!("{response}");
                    Ok(())
                }
                Err(_) if matches!(action, ReceiverServerAction::Status) => {
                    println!("receiver server is stopped (no brain TUI is running)");
                    Ok(())
                }
                Err(_) => anyhow::bail!(
                    "the receiver server belongs to the running brain TUI; use `brain --with-receiver` or the command palette"
                ),
            }
        }
    }
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

fn receiver_setup(root: &std::path::Path) -> Result<()> {
    let theme = crate::theme::Theme::active();
    println!("{}", theme.heading("Set up the brain receiver"));
    println!("{}", theme.muted("Choose which channels to configure:"));
    println!("  {}", theme.accent("1) Email"));
    println!("  {}", theme.accent("2) SMS"));
    println!("  {}", theme.accent("3) Both"));
    let Some(channel_input) = super::super::configuration::prompt_tty_line(&format!(
        "{} ",
        theme.prompt("Choose 1, 2, or 3:")
    ))?
    else {
        anyhow::bail!("receiver setup needs an interactive terminal; nothing was changed");
    };
    let Some(channels) = parse_receiver_channels(channel_input.trim()) else {
        anyhow::bail!("choose 1 for email, 2 for SMS, or 3 for both");
    };
    println!(
        "{}",
        theme.muted("Press Enter to keep an existing value. Type /clear to erase it.")
    );
    for name in receiver_provider_fields(channels) {
        let (label, description, secret) = receiver_provider_prompt(name);
        println!("{}", theme.muted(description));
        prompt_receiver_value(name, label, secret)?;
    }
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
            "Phone numbers allowed to text Brain (E.164, comma-separated)",
            "Include + and the country code, for example +16072809118. Messages from any other number are rejected before they reach the LLM.",
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
        let Some(input) = super::super::configuration::prompt_tty_line(&format!(
            "{} {}: ",
            theme.prompt(label),
            hint
        ))?
        else {
            anyhow::bail!("receiver setup needs an interactive terminal; nothing was changed");
        };
        let value = match input.trim() {
            "" => old,
            "/clear" => String::new(),
            value => value.to_owned(),
        };
        crate::settings::set(name, &value)?;
    }
    install_receiver_hooks(root)?;
    println!("{}", theme.success("receiver configuration saved"));
    let public_url = crate::env::get("brain_receiver_public_url").unwrap_or_default();
    if channels.sms() {
        println!(
            "{}",
            theme.muted(&format!(
                "Twilio webhook URL: {}",
                receiver_webhook_url(&public_url, "sms")
            ))
        );
    }
    if channels.email() {
        println!(
            "{}",
            theme.muted(&format!(
                "Resend webhook URL: {}",
                receiver_webhook_url(&public_url, "email")
            ))
        );
    }
    Ok(())
}

fn parse_receiver_channels(input: &str) -> Option<ReceiverSetupChannels> {
    match input {
        "1" => Some(ReceiverSetupChannels::Email),
        "2" => Some(ReceiverSetupChannels::Sms),
        "3" => Some(ReceiverSetupChannels::Both),
        _ => None,
    }
}

fn receiver_webhook_url(public_base_url: &str, channel: &str) -> String {
    format!(
        "{}/{}",
        public_base_url.trim_end_matches('/'),
        channel.trim_start_matches('/')
    )
}

fn receiver_provider_fields(channels: ReceiverSetupChannels) -> Vec<&'static str> {
    let mut fields = vec!["brain_receiver_public_url"];
    if channels.sms() {
        fields.extend([
            "twilio_account_sid",
            "twilio_auth_token",
            "twilio_from_number",
        ]);
    }
    if channels.email() {
        fields.extend([
            "resend_api_key",
            "resend_from_email",
            "resend_webhook_signing_secret",
        ]);
    }
    fields
}

fn receiver_provider_prompt(name: &str) -> (&'static str, &'static str, bool) {
    match name {
        "brain_receiver_public_url" => (
            "Public base URL",
            "Enter the public base URL. Brain derives /sms and /email webhook paths from it.",
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
            "The Twilio phone number Brain uses for outbound SMS. Include + and the country code, for example +16072809118.",
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

fn prompt_receiver_value(name: &str, label: &str, secret: bool) -> Result<()> {
    let old = crate::env::get(name).unwrap_or_default();
    let hint = if old.trim().is_empty() {
        "(not set)"
    } else {
        "(saved)"
    };
    let prompt = format!("{} {}: ", crate::theme::Theme::active().prompt(label), hint);
    let input = if secret {
        super::super::configuration::prompt_masked_line(&prompt)?
    } else {
        super::super::configuration::prompt_tty_line(&prompt)?
    }
    .ok_or_else(|| anyhow::anyhow!("receiver setup needs an interactive terminal"))?;
    let value = match input.trim() {
        "" => old,
        "/clear" => String::new(),
        value => value.to_owned(),
    };
    crate::env::set(name, &value)
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
        let (label, description, secret) = receiver_provider_prompt(name);
        (name, label, description, secret)
    })
    .collect()
}

fn receiver_set(assignment: Option<&str>) -> Result<()> {
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
            super::super::configuration::prompt_tty_line("Choose a variable number: ")?
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
        return crate::env::set(&name, value);
    }
    prompt_receiver_value(&name, label, *secret)
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
                items.iter().any(|item| {
                    item.get("command").and_then(serde_json::Value::as_str) == Some(command)
                })
            })
    });
    if !exists {
        list.push(serde_json::json!({"hooks": [{"type": "command", "command": command}]}));
    }
}

fn hook_command(hook_path: &std::path::Path, home: &std::path::Path) -> String {
    hook_path.strip_prefix(home).map_or_else(
        |_| format!("python3 {}", hook_path.to_string_lossy()),
        |relative| format!("python3 ~/{}", relative.to_string_lossy()),
    )
}

fn install_receiver_hooks(root: &std::path::Path) -> Result<()> {
    let hook_dir = root.join(".claude").join("brain-hooks");
    std::fs::create_dir_all(&hook_dir)?;
    let session_path = hook_dir.join("claude_session_start_hook.py");
    let stop_path = hook_dir.join("claude_stop_hook.py");
    std::fs::write(
        &session_path,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/claude_session_start_hook.py"
        )),
    )?;
    std::fs::write(
        &stop_path,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/claude_stop_hook.py"
        )),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&session_path, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&stop_path, std::fs::Permissions::from_mode(0o755))?;
    }
    let home = std::path::PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?,
    );
    let session = hook_command(&session_path, &home);
    let stop = hook_command(&stop_path, &home);
    let settings_path = root.join(".claude/settings.json");
    let mut settings = if settings_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&settings_path)?)?
    } else {
        serde_json::json!({})
    };
    ensure_hook_entry(&mut settings, "SessionStart", &session);
    ensure_hook_entry(&mut settings, "Stop", &stop);
    std::fs::write(settings_path, serde_json::to_vec_pretty(&settings)?)?;
    let codex_dir = home.join(".codex");
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
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{
        ReceiverSetupChannels, ensure_hook_entry, hook_command, parse_receiver_channels,
        receiver_provider_fields, receiver_webhook_url,
    };

    #[test]
    fn hook_command_is_home_relative_for_paths_under_home() {
        let command = hook_command(
            Path::new("/Users/pablo/brain/.claude/brain-hooks/claude_stop_hook.py"),
            Path::new("/Users/pablo"),
        );
        assert_eq!(
            command,
            "python3 ~/brain/.claude/brain-hooks/claude_stop_hook.py"
        );
    }

    #[test]
    fn hook_command_falls_back_to_absolute_outside_home() {
        assert_eq!(
            hook_command(Path::new("/opt/hooks/x.py"), Path::new("/Users/pablo")),
            "python3 /opt/hooks/x.py"
        );
    }

    #[test]
    fn hook_command_is_identical_across_machines_with_different_homes() {
        let mini = hook_command(
            Path::new("/Users/pablo/brain/.claude/brain-hooks/claude_stop_hook.py"),
            Path::new("/Users/pablo"),
        );
        let mbp = hook_command(
            Path::new("/Users/juanpablosarmiento/brain/.claude/brain-hooks/claude_stop_hook.py"),
            Path::new("/Users/juanpablosarmiento"),
        );
        assert_eq!(mini, mbp);
    }

    #[test]
    fn hook_merge_is_idempotent_and_preserves_other_settings() {
        let mut settings = json!({"permissions": {"allow": ["Read"]}});
        ensure_hook_entry(&mut settings, "SessionStart", "/tmp/session.py");
        ensure_hook_entry(&mut settings, "SessionStart", "/tmp/session.py");
        assert_eq!(
            settings["hooks"]["SessionStart"].as_array().unwrap().len(),
            1
        );
        assert_eq!(settings["permissions"]["allow"][0], "Read");
    }

    #[test]
    fn channel_menu_selects_only_the_requested_configuration() {
        assert_eq!(
            parse_receiver_channels("1"),
            Some(ReceiverSetupChannels::Email)
        );
        assert_eq!(
            parse_receiver_channels("2"),
            Some(ReceiverSetupChannels::Sms)
        );
        assert_eq!(
            parse_receiver_channels("3"),
            Some(ReceiverSetupChannels::Both)
        );
        assert_eq!(parse_receiver_channels("4"), None);
    }

    #[test]
    fn public_base_url_expands_to_exact_webhook_endpoints() {
        assert_eq!(
            receiver_webhook_url("https://brain.example.com/", "sms"),
            "https://brain.example.com/sms"
        );
        assert_eq!(
            receiver_webhook_url("https://brain.example.com", "email"),
            "https://brain.example.com/email"
        );
    }

    #[test]
    fn provider_setup_fields_follow_selected_channels() {
        assert_eq!(
            receiver_provider_fields(ReceiverSetupChannels::Sms),
            [
                "brain_receiver_public_url",
                "twilio_account_sid",
                "twilio_auth_token",
                "twilio_from_number",
            ]
        );
        assert_eq!(
            receiver_provider_fields(ReceiverSetupChannels::Email),
            [
                "brain_receiver_public_url",
                "resend_api_key",
                "resend_from_email",
                "resend_webhook_signing_secret",
            ]
        );
    }
}
