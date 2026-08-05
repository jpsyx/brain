//! Receiver setup and lifecycle command dispatch.

use anyhow::Result;

mod enablement;
mod hooks;

pub(crate) use enablement::{
    ReceiverIntentRefresher, apply_receiver_action_with, apply_startup_receiver_flag,
    read_receiver_status, receiver_enabled,
};
#[cfg(test)]
use enablement::{ReceiverStatus, apply_startup_receiver_flag_with, receiver_status};
use enablement::{print_receiver_change, print_receiver_status};

/// Refresh the bundled lifecycle hooks before an agent-capable TUI starts.
pub(crate) fn refresh_agent_hooks(root: &std::path::Path) -> Result<()> {
    hooks::install(root)
}

pub fn run_receiver(
    args: &crate::cli::ReceiverArgs,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    run_receiver_with_refresher(
        args,
        context,
        &crate::server::control::ServerClient::default(),
    )
}

pub(crate) fn run_receiver_with_refresher(
    args: &crate::cli::ReceiverArgs,
    context: &crate::workspace::CommandContext,
    refresher: &dyn ReceiverIntentRefresher,
) -> Result<()> {
    use crate::cli::ReceiverServerAction;
    match &args.action {
        ReceiverServerAction::Setup => receiver_setup(context),
        ReceiverServerAction::Set { assignment } => receiver_set(context, assignment.as_deref()),
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
        ReceiverServerAction::Logs => crate::server::lifecycle::logs(),
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

fn receiver_setup(context: &crate::workspace::CommandContext) -> Result<()> {
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
        prompt_receiver_value(context, name, label, secret)?;
    }
    let current = crate::config::Config::load(&context.workspace);
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
        crate::settings::set(&context.workspace, name, &value)?;
    }
    hooks::install(context.workspace.root())?;
    println!("{}", theme.success("receiver configuration saved"));
    let public_url = crate::env::get(context, "brain_receiver_public_url").unwrap_or_default();
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
    crate::env::set(context, name, &value)
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
        return crate::env::set(context, &name, value);
    }
    prompt_receiver_value(context, &name, label, *secret)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::{
        ReceiverIntentRefresher, ReceiverSetupChannels, ReceiverStatus, apply_receiver_action_with,
        apply_startup_receiver_flag_with, parse_receiver_channels, receiver_provider_fields,
        receiver_status, receiver_webhook_url, run_receiver_with_refresher,
    };
    use crate::workspace::{
        CommandContext, MachineRegistry, ReceiverAction, RegistryStore, WorkspaceContext,
        WorkspaceId, WorkspaceName, WorkspaceRecord,
    };

    struct FailedRefresh;

    impl ReceiverIntentRefresher for FailedRefresh {
        fn refresh_enabled(&self, _workspace_id: WorkspaceId) -> anyhow::Result<()> {
            anyhow::bail!("control socket disappeared")
        }
    }

    #[derive(Clone)]
    struct RecordingRefresh(Arc<std::sync::Mutex<Vec<WorkspaceId>>>);

    impl ReceiverIntentRefresher for RecordingRefresh {
        fn refresh_enabled(&self, workspace_id: WorkspaceId) -> anyhow::Result<()> {
            self.0.lock().unwrap().push(workspace_id);
            Ok(())
        }
    }

    #[test]
    fn status_requires_persistent_intent_and_an_enabled_exact_lease_to_accept() {
        assert_eq!(
            receiver_status(true, true, Some(false)),
            ReceiverStatus {
                enabled: true,
                tui_live: true,
                server_running: true,
                accepting: false,
            }
        );
        assert_eq!(
            receiver_status(true, true, Some(true)),
            ReceiverStatus {
                enabled: true,
                tui_live: true,
                server_running: true,
                accepting: true,
            }
        );
        assert_eq!(
            receiver_status(true, false, None),
            ReceiverStatus {
                enabled: true,
                tui_live: false,
                server_running: false,
                accepting: false,
            }
        );
    }

    #[test]
    fn cli_start_stop_and_startup_flag_drive_exact_persistence_and_refresh() {
        let temporary = tempfile::tempdir().expect("receiver fixture");
        let personal_name = WorkspaceName::parse("personal").unwrap();
        let family_name = WorkspaceName::parse("family").unwrap();
        let personal_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
        let family_id = WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
        let family_root = temporary.path().join("family");
        let store = RegistryStore::from_path(temporary.path().join("env.json"));
        store
            .replace(&MachineRegistry {
                schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
                default_workspace: personal_name.clone(),
                workspaces: BTreeMap::from([
                    (
                        personal_name.clone(),
                        WorkspaceRecord {
                            workspace_id: personal_id,
                            root: temporary.path().join("personal"),
                            aliases: BTreeSet::new(),
                            local_user_id: "personal-user".to_owned(),
                            receiver_enabled: false,
                            env: serde_json::Map::new(),
                        },
                    ),
                    (
                        family_name.clone(),
                        WorkspaceRecord {
                            workspace_id: family_id,
                            root: family_root.clone(),
                            aliases: BTreeSet::new(),
                            local_user_id: "family-user".to_owned(),
                            receiver_enabled: false,
                            env: serde_json::Map::new(),
                        },
                    ),
                ]),
            })
            .unwrap();
        let workspace = WorkspaceContext::new(
            temporary.path(),
            family_id,
            family_name.clone(),
            &family_root,
            "family-user",
            temporary.path(),
        )
        .unwrap();
        let context = CommandContext::for_test(Arc::new(workspace), store.clone(), "family-user");
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let refresher = RecordingRefresh(Arc::clone(&calls));

        for (command, expected) in [("start", true), ("stop", false)] {
            let cli = crate::cli::try_parse_from(["brain", "-b", "family", "receiver", command])
                .expect("parse receiver command");
            let Some(crate::cli::Cmd::Receiver(args)) = cli.command else {
                panic!("receiver command");
            };
            run_receiver_with_refresher(&args, &context, &refresher).unwrap();
            let saved = RegistryStore::load_from(store.path()).unwrap();
            assert_eq!(saved.workspaces[&family_name].receiver_enabled, expected);
            assert!(!saved.workspaces[&personal_name].receiver_enabled);
        }

        let cli = crate::cli::try_parse_from(["brain", "--with-receiver", "-b", "family"])
            .expect("parse startup flag");
        apply_startup_receiver_flag_with(cli.with_receiver, &context, &refresher).unwrap();
        let saved = RegistryStore::load_from(store.path()).unwrap();
        assert!(saved.workspaces[&family_name].receiver_enabled);
        assert!(!saved.workspaces[&personal_name].receiver_enabled);
        assert_eq!(*calls.lock().unwrap(), [family_id, family_id, family_id]);
    }

    #[test]
    fn committed_intent_survives_a_failed_live_refresh() {
        let temporary = tempfile::tempdir().expect("receiver fixture");
        let name = WorkspaceName::parse("personal").expect("workspace name");
        let workspace_id =
            WorkspaceId::parse("2174fb9d-ae76-4bde-a526-38ac43ebdf8f").expect("workspace ID");
        let root = temporary.path().join("personal");
        let store = RegistryStore::from_path(temporary.path().join("env.json"));
        store
            .replace(&MachineRegistry {
                schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
                default_workspace: name.clone(),
                workspaces: BTreeMap::from([(
                    name.clone(),
                    WorkspaceRecord {
                        workspace_id,
                        root: root.clone(),
                        aliases: BTreeSet::new(),
                        local_user_id: "tester".to_owned(),
                        receiver_enabled: false,
                        env: serde_json::Map::new(),
                    },
                )]),
            })
            .expect("seed registry");
        let workspace = WorkspaceContext::new(
            temporary.path(),
            workspace_id,
            name,
            &root,
            "tester",
            &PathBuf::from("/"),
        )
        .expect("workspace context");
        let context = CommandContext::for_test(Arc::new(workspace), store.clone(), "tester");

        let outcome = apply_receiver_action_with(&context, ReceiverAction::Start, &FailedRefresh)
            .expect("persistence success must remain success");

        assert!(outcome.enabled());
        assert!(outcome.refresh_warning().is_some());
        let saved = RegistryStore::load_from(store.path()).expect("saved registry");
        assert!(saved.workspaces[&WorkspaceName::parse("personal").unwrap()].receiver_enabled);
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
