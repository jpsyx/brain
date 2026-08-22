use super::{
    Arc, BootstrapContext, BufRead, CommandContext, Context, InteractionMode, Path, ReadinessField,
    RegistryStore, Result, WorkspaceContext, WorkspaceManifest, Write, anyhow,
    readiness_action_with_users,
};

pub(super) fn adopt_local_user(
    store: &RegistryStore,
    selector: &str,
    expected_workspace_id: crate::workspace::WorkspaceId,
    user_id: &crate::users::UserId,
) -> Result<()> {
    store.transaction(|transaction| -> Result<()> {
        let mut registry = transaction.load()?;
        let selected = registry.select(Some(selector))?;
        if selected.record().workspace_id != expected_workspace_id {
            anyhow::bail!("selected workspace identity changed before adopting the local user");
        }
        let canonical_name = selected.canonical_name().clone();
        let users = crate::users::UsersStore::load_from(
            &selected.record().root.join(".config/users.json"),
        )?;
        if users.user(user_id).is_none() {
            anyhow::bail!("sole workspace user {user_id} is no longer a portable member");
        }
        transaction.update(&mut registry, |candidate| {
            let target = &mut candidate
                .workspaces
                .get_mut(&canonical_name)
                .expect("selected workspace remains present")
                .local_user_id;
            user_id.to_string().clone_into(target);
            Ok(())
        })?;
        Ok(())
    })?;
    let theme = crate::theme::Theme::active();
    eprintln!(
        "{} {}",
        theme.info("Linked this machine to your only workspace user"),
        theme.value(user_id.as_str())
    );
    Ok(())
}

pub(super) fn ensure_selected_registry_access_mode(
    store: &RegistryStore,
    selector: Option<&str>,
) -> Result<()> {
    match RegistryStore::load_from(store.path()) {
        Ok(registry) => {
            let selected = registry.select(selector)?;
            if !selected.record().root.is_dir() {
                anyhow::bail!(
                    "workspace root {} is unavailable; cannot validate portable access mode",
                    selected.record().root.display()
                );
            }
            let access_mode = if selected.canonical_name() == &registry.default_workspace {
                crate::access::AccessMode::Unrestricted
            } else {
                crate::access::AccessMode::WorkspaceOnly
            };
            crate::access::ensure_portable_access_mode(&selected.record().root, access_mode)
                .map_err(|error| anyhow!("validate portable workspace access mode: {error:#}"))
        }
        Err(crate::workspace::RegistryError::Io {
            operation: crate::workspace::RegistryOperation::ReadRegistry,
            kind: std::io::ErrorKind::NotFound,
            ..
        }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Ask which portable member this machine is.
///
/// A roster that cannot be read, or one with nobody in it, falls back to the
/// plain ID prompt: readiness repair is the last thing that should fail, and
/// the transaction still rejects an ID that is not a member.
fn prompt_local_user(
    root: &Path,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<String> {
    let theme = crate::theme::Theme::active();
    let choices = crate::users::UsersStore::load_from(&root.join(".config/users.json"))
        .map(|users| crate::users::local_user_choices(&users))
        .unwrap_or_default();
    if choices.is_empty() {
        return crate::workspace::command::prompt::read_required(
            writer,
            reader,
            crate::workspace::command::prompt::PromptField::LocalUserId,
            theme,
        );
    }
    crate::workspace::command::prompt::read_local_user(writer, reader, &choices, theme)
}

pub(super) fn repair_interactively(
    store: &RegistryStore,
    selector: &str,
    expected_workspace_id: crate::workspace::WorkspaceId,
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
        Some(prompt_local_user(
            &prompt_selected.record().root,
            reader,
            writer,
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

pub(super) fn repaired_context(
    store: &RegistryStore,
    canonical_name: &crate::workspace::WorkspaceName,
    expected_workspace_id: crate::workspace::WorkspaceId,
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

pub(super) fn prompt_first_user(
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
    let response_email = crate::users::normalize_email(&config.response_email).ok();
    let email_default = response_email
        .as_deref()
        .filter(|response| {
            allowed_emails.iter().any(|allowed| {
                crate::users::normalize_email(allowed)
                    .is_ok_and(|normalized| normalized == *response)
            })
        })
        .or_else(|| (allowed_emails.len() == 1).then(|| allowed_emails[0].as_str()));
    let email = if allowed_emails.is_empty() {
        None
    } else {
        let value = read_prompt_value(reader, writer, "Email", email_default, theme)?;
        Some(crate::users::EmailIdentity {
            value: crate::users::normalize_email(&value)
                .map_err(|_| anyhow!("email `{value}` is invalid"))?,
            inbound_allowed: true,
        })
    };
    let response_email = email
        .as_ref()
        .and_then(|email| response_email.filter(|response| response == &email.value));
    Ok(crate::users::User {
        id,
        name,
        phones: phone.into_iter().collect(),
        emails: email.into_iter().collect(),
        response_email,
    })
}

pub(super) fn read_prompt_value(
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

pub(super) fn context_from_record(
    store: &RegistryStore,
    canonical_name: crate::workspace::WorkspaceName,
    record: &crate::workspace::WorkspaceRecord,
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
    Ok(BootstrapContext::Ready(CommandContext::new(
        Arc::new(workspace),
        store.clone(),
    )?))
}
