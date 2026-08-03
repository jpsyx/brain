//! Command bootstrap policy and selected workspace construction.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use super::bootstrap_policy::{RegistryOnlyPromptOrder, registry_only_prompt_order};
use super::{
    BootstrapPolicy, InteractionMode, Invocation, ReadinessAction, ReadinessField, RegistryStore,
    WorkspaceContext, WorkspaceManifest, bootstrap_policy, invocation_for,
    readiness_action_with_users,
};

/// One ready selected workspace plus the machine registry capability.
#[derive(Debug, Clone)]
pub struct CommandContext {
    pub workspace: Arc<WorkspaceContext>,
    pub registry_store: RegistryStore,
}

/// Bootstrap capability returned to top-level dispatch.
#[derive(Debug, Clone)]
pub enum BootstrapContext {
    None,
    RegistryOnly(RegistryStore),
    Ready(CommandContext),
}

/// Bootstrap one real process invocation.
pub fn bootstrap(cli: &mut crate::cli::Cli) -> Result<BootstrapContext> {
    let policy = bootstrap_policy(invocation_for(cli));
    if matches!(
        policy,
        BootstrapPolicy::None | BootstrapPolicy::InternalNoPrompt
    ) {
        return Ok(BootstrapContext::None);
    }
    let store = RegistryStore::real();
    if policy == BootstrapPolicy::RegistryOnly {
        return registry_only_bootstrap_with(
            cli,
            store,
            crate::workspace::command::preflight_registry_only,
            |prepared| {
                let invocation = invocation_for(prepared);
                debug_assert_eq!(
                    registry_only_prompt_order(invocation),
                    Some(RegistryOnlyPromptOrder::BeforeMigration)
                );
                let should_migrate = match invocation {
                    Invocation::WorkspaceCreate | Invocation::WorkspaceAttach => {
                        crate::env::registry_setup_needs_migration()?
                    }
                    Invocation::WorkspaceRemove | Invocation::WorkspaceRepair => {
                        !crate::env::registry_is_valid_v2()?
                    }
                    Invocation::User => !crate::env::registry_is_valid_v2()?,
                    _ => false,
                };
                if should_migrate {
                    crate::env::migrate_checked()?;
                }
                Ok(())
            },
        );
    }

    if !crate::env::registry_is_valid_v2()? {
        crate::env::migrate_checked()?;
    }
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))?;
    let current_dir = std::env::current_dir().context("read current directory")?;
    let interaction = if std::io::stdin().is_terminal() {
        InteractionMode::Interactive
    } else {
        InteractionMode::NonInteractive
    };
    if interaction == InteractionMode::Interactive {
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("open /dev/tty for workspace readiness")?;
        let mut writer = tty.try_clone().context("clone readiness terminal")?;
        let mut reader = BufReader::new(tty);
        bootstrap_with_io(
            cli,
            store,
            &home,
            &current_dir,
            interaction,
            &mut reader,
            &mut writer,
        )
    } else {
        bootstrap_with_io(
            cli,
            store,
            &home,
            &current_dir,
            interaction,
            &mut std::io::empty(),
            &mut std::io::sink(),
        )
    }
}

/// Bootstrap against injected paths and terminal IO.
pub fn bootstrap_with_io(
    cli: &mut crate::cli::Cli,
    store: RegistryStore,
    home: &Path,
    current_dir: &Path,
    interaction: InteractionMode,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<BootstrapContext> {
    if bootstrap_policy(invocation_for(cli)) == BootstrapPolicy::RegistryOnly {
        crate::workspace::command::preflight_registry_only_with_io(
            cli,
            reader,
            writer,
            crate::theme::Theme::active(),
        )?;
        return Ok(BootstrapContext::RegistryOnly(store));
    }
    bootstrap_with_io_and_hook(
        cli,
        store,
        (home, current_dir),
        interaction,
        reader,
        writer,
        || Ok(()),
    )
}

fn registry_only_bootstrap_with(
    cli: &mut crate::cli::Cli,
    store: RegistryStore,
    preflight: impl FnOnce(&mut crate::cli::Cli) -> Result<()>,
    after_preflight: impl FnOnce(&crate::cli::Cli) -> Result<()>,
) -> Result<BootstrapContext> {
    preflight(cli)?;
    after_preflight(cli)?;
    Ok(BootstrapContext::RegistryOnly(store))
}

fn bootstrap_with_io_and_hook(
    cli: &crate::cli::Cli,
    store: RegistryStore,
    paths: (&Path, &Path),
    interaction: InteractionMode,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    after_readiness: impl FnOnce() -> Result<()>,
) -> Result<BootstrapContext> {
    let (home, current_dir) = paths;
    let policy = bootstrap_policy(invocation_for(cli));
    match policy {
        BootstrapPolicy::None | BootstrapPolicy::InternalNoPrompt => {
            return Ok(BootstrapContext::None);
        }
        BootstrapPolicy::RegistryOnly => return Ok(BootstrapContext::RegistryOnly(store)),
        BootstrapPolicy::ReadyWorkspace => {}
    }

    let registry = RegistryStore::load_from(store.path())?;
    let selected = registry.select(cli.brain.as_deref())?;
    let canonical_name = selected.canonical_name().clone();
    let workspace_id = selected.record().workspace_id;
    let record = selected.record().clone();
    if !selected.record().root.is_dir() {
        anyhow::bail!(
            "workspace root {} is unavailable; restore it or detach the workspace",
            selected.record().root.display()
        );
    }
    let provisional = WorkspaceContext::new(
        home,
        record.workspace_id,
        canonical_name.clone(),
        &record.root,
        record.local_user_id.clone(),
        current_dir,
    )?;
    let manifest = WorkspaceManifest::load(&selected.record().root, env!("CARGO_PKG_VERSION"));
    let users = crate::users::UsersStore::load(&provisional);
    let action = readiness_action_with_users(
        selected.canonical_name(),
        selected.record(),
        manifest,
        users,
        interaction,
    )?;
    match action {
        ReadinessAction::Ready(_) => {
            after_readiness()?;
            context_from_record(&store, canonical_name, &record, home, current_dir)
        }
        ReadinessAction::Prompt(fields) => {
            repair_interactively(
                &store,
                canonical_name.as_str(),
                workspace_id,
                &fields,
                reader,
                writer,
            )?;
            after_readiness()?;
            repaired_context(&store, &canonical_name, workspace_id, home, current_dir)
        }
    }
}

fn repair_interactively(
    store: &RegistryStore,
    selector: &str,
    expected_workspace_id: super::WorkspaceId,
    fields: &[ReadinessField],
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<()> {
    let prompt_registry = RegistryStore::load_from(store.path())?;
    let prompt_selected = prompt_registry.select(Some(selector))?;
    if prompt_selected.record().workspace_id != expected_workspace_id {
        anyhow::bail!("selected workspace identity changed before readiness prompts");
    }
    let new_user = if fields.contains(&ReadinessField::PortableUsers) {
        Some(prompt_first_user(
            &prompt_selected.record().root,
            reader,
            writer,
        )?)
    } else {
        None
    };
    let local_user_id = if let Some(user) = new_user.as_ref() {
        Some(user.id.to_string())
    } else if fields.contains(&ReadinessField::LocalUserId) {
        Some(super::command::prompt::read_required(
            writer,
            reader,
            super::command::prompt::PromptField::LocalUserId,
            crate::theme::Theme::active(),
        )?)
    } else {
        None
    };
    store.transaction(|transaction| -> Result<()> {
        let mut registry = transaction.load()?;
        let selected = registry.select(Some(selector))?;
        if selected.record().workspace_id != expected_workspace_id {
            anyhow::bail!("selected workspace identity changed during readiness repair");
        }
        let canonical_name = selected.canonical_name().clone();
        let root = selected.record().root.clone();
        let workspace_id = selected.record().workspace_id;
        if fields.contains(&ReadinessField::Manifest) {
            WorkspaceManifest::new(workspace_id).write_new(&root)?;
        }
        if let Some(user) = new_user.clone() {
            let mut users = crate::users::Users::empty();
            crate::users::apply_mutation(&mut users, crate::users::UserMutation::Add(user))?;
            crate::users::UsersStore::save_to(&root.join(".config/users.json"), &users)?;
        }
        if let Some(local_user_id) = local_user_id.as_deref() {
            if new_user.is_none() {
                let users = crate::users::UsersStore::load_from(&root.join(".config/users.json"))?;
                let id = crate::users::UserId::parse(local_user_id)?;
                if users.user(&id).is_none() {
                    anyhow::bail!("local user {id} is not a portable member");
                }
            }
            transaction.update(&mut registry, |candidate| {
                let target = &mut candidate
                    .workspaces
                    .get_mut(&canonical_name)
                    .expect("selected workspace remains present")
                    .local_user_id;
                local_user_id.clone_into(target);
                Ok(())
            })?;
        }
        Ok(())
    })
}

fn repaired_context(
    store: &RegistryStore,
    canonical_name: &super::WorkspaceName,
    expected_workspace_id: super::WorkspaceId,
    home: &Path,
    current_dir: &Path,
) -> Result<BootstrapContext> {
    let registry = RegistryStore::load_from(store.path())?;
    let selected = registry.select(Some(canonical_name.as_str()))?;
    if selected.record().workspace_id != expected_workspace_id {
        anyhow::bail!("selected workspace identity changed during command bootstrap");
    }
    let manifest = WorkspaceManifest::load(&selected.record().root, env!("CARGO_PKG_VERSION"))?;
    let provisional = WorkspaceContext::new(
        home,
        selected.record().workspace_id,
        selected.canonical_name().clone(),
        &selected.record().root,
        selected.record().local_user_id.clone(),
        current_dir,
    )?;
    readiness_action_with_users(
        selected.canonical_name(),
        selected.record(),
        Ok(manifest),
        crate::users::UsersStore::load(&provisional),
        InteractionMode::NonInteractive,
    )?;
    context_from_record(
        store,
        selected.canonical_name().clone(),
        selected.record(),
        home,
        current_dir,
    )
}

fn prompt_first_user(
    root: &Path,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<crate::users::User> {
    let theme = crate::theme::Theme::active();
    let name = read_prompt_value(reader, writer, "Your display name", None, theme)?;
    let proposed = crate::users::proposed_user_id(&name);
    let id = loop {
        let value = read_prompt_value(reader, writer, "User ID", Some(&proposed), theme)?;
        match crate::users::UserId::parse(&value) {
            Ok(id) => break id,
            Err(error) => writeln!(writer, "{}", theme.warning(&error.to_string()))?,
        }
    };
    let config: crate::config::Config = serde_json::from_value(serde_json::Value::Object(
        crate::settings::load_map_at(&root.join(".config/config.json")),
    ))
    .unwrap_or_default();
    let allowed_phones = config.allowed_sms();
    let allowed_emails = config.allowed_email();
    let phone = if allowed_phones.is_empty() {
        None
    } else {
        let default = (allowed_phones.len() == 1).then(|| allowed_phones[0].as_str());
        let value = read_prompt_value(reader, writer, "Phone", default, theme)?;
        Some(crate::users::PhoneIdentity {
            value: crate::users::normalize_phone(&value)
                .map_err(|_| anyhow!("phone `{value}` is invalid"))?,
            inbound_allowed: true,
        })
    };
    let email_default = if !config.response_email.trim().is_empty() {
        Some(config.response_email.trim())
    } else if allowed_emails.len() == 1 {
        Some(allowed_emails[0].as_str())
    } else {
        None
    };
    let email = if allowed_emails.is_empty() && email_default.is_none() {
        None
    } else {
        let value = read_prompt_value(reader, writer, "Email", email_default, theme)?;
        Some(crate::users::EmailIdentity {
            value: crate::users::normalize_email(&value)
                .map_err(|_| anyhow!("email `{value}` is invalid"))?,
            inbound_allowed: true,
        })
    };
    let response_email = email.as_ref().and_then(|email| {
        crate::users::normalize_email(&config.response_email)
            .ok()
            .filter(|response| response == &email.value)
    });
    Ok(crate::users::User {
        id,
        name,
        phones: phone.into_iter().collect(),
        emails: email.into_iter().collect(),
        response_email,
    })
}

fn read_prompt_value(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    label: &str,
    default: Option<&str>,
    theme: crate::theme::Theme,
) -> Result<String> {
    loop {
        let prompt = default.map_or_else(
            || format!("{label}:"),
            |default| format!("{label} [{default}]:"),
        );
        write!(writer, "{} ", theme.prompt(&prompt)).context("write user readiness prompt")?;
        writer.flush().context("flush user readiness prompt")?;
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .context("read user readiness prompt")?
            == 0
        {
            anyhow::bail!("workspace setup cancelled before portable user creation");
        }
        let value = line.trim();
        if !value.is_empty() {
            return Ok(value.to_owned());
        }
        if let Some(default) = default
            && !default.is_empty()
        {
            return Ok(default.to_owned());
        }
        writeln!(writer, "{}", theme.warning("A value is required."))?;
    }
}

fn context_from_record(
    store: &RegistryStore,
    canonical_name: super::WorkspaceName,
    record: &super::WorkspaceRecord,
    home: &Path,
    current_dir: &Path,
) -> Result<BootstrapContext> {
    let workspace = WorkspaceContext::new(
        home,
        record.workspace_id,
        canonical_name,
        &record.root,
        record.local_user_id.clone(),
        current_dir,
    )?;
    Ok(BootstrapContext::Ready(CommandContext {
        workspace: Arc::new(workspace),
        registry_store: store.clone(),
    }))
}

#[cfg(test)]
#[path = "bootstrap/tests.rs"]
mod tests;
