//! Portable user mapping collected by receiver setup.

use anyhow::{Context as _, Result};

use crate::cli::{ReceiverSetupArgs, ReceiverSetupChannels};
use crate::users::{EmailIdentity, PhoneIdentity, User, UserId, Users, UsersStore};
use crate::workspace::WorkspaceContext;

pub(super) fn headless_plan(
    args: &ReceiverSetupArgs,
    workspace: &WorkspaceContext,
    channels: ReceiverSetupChannels,
) -> Result<Users> {
    let id = args
        .user_id
        .as_deref()
        .context("receiver setup requires --user-id")?;
    let phone = super::sms(channels)
        .then(|| {
            let value = args
                .phone
                .as_deref()
                .context("SMS receiver setup requires --phone")?;
            let allowed = args
                .phone_allowed
                .context("SMS receiver setup requires --phone-allowed true|false")?;
            Ok::<_, anyhow::Error>((value.to_owned(), allowed))
        })
        .transpose()?;
    let email = super::email(channels)
        .then(|| {
            let value = args
                .email
                .as_deref()
                .context("email receiver setup requires --email")?;
            let allowed = args
                .email_allowed
                .context("email receiver setup requires --email-allowed true|false")?;
            Ok::<_, anyhow::Error>((value.to_owned(), allowed))
        })
        .transpose()?;
    apply_user_mapping(
        load_or_empty(workspace)?,
        id,
        args.user_name.as_deref(),
        phone,
        email,
        args.response_email.as_deref(),
    )
}

pub(super) fn interactive_plan(
    workspace: &WorkspaceContext,
    channels: ReceiverSetupChannels,
) -> Result<Users> {
    let users = load_or_empty(workspace)?;
    let theme = crate::theme::Theme::active();
    println!(
        "{}",
        theme.heading("Map receiver addresses to a portable user")
    );
    if !users.users.is_empty() {
        println!("{}", theme.muted("Portable users in this workspace:"));
        for user in &users.users {
            println!("  {}  {}", theme.accent(user.id.as_str()), user.name);
        }
    }
    let id = prompt("Existing user ID, or a new user ID:")?;
    let parsed_id = UserId::parse(&id)?;
    let name = (mapping_choice(&users, &parsed_id) == MappingChoice::Create)
        .then(|| prompt("Display name for the new user:"))
        .transpose()?;
    let fields = interactive_fields(channels);
    let phone = fields
        .contains(&"phone")
        .then(|| {
            Ok::<_, anyhow::Error>((
                prompt("Phone address (E.164):")?,
                prompt_allowed("Allow this phone to initiate inbound work? [Y/n]:")?,
            ))
        })
        .transpose()?;
    let email = fields
        .contains(&"email")
        .then(|| {
            Ok::<_, anyhow::Error>((
                prompt("Email address:")?,
                prompt_allowed("Allow this email to initiate inbound work? [Y/n]:")?,
            ))
        })
        .transpose()?;
    apply_user_mapping(users, &id, name.as_deref(), phone, email, None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MappingChoice {
    Existing,
    Create,
}

fn mapping_choice(users: &Users, id: &UserId) -> MappingChoice {
    if users.user(id).is_some() {
        MappingChoice::Existing
    } else {
        MappingChoice::Create
    }
}

fn interactive_fields(channels: ReceiverSetupChannels) -> Vec<&'static str> {
    let mut fields = Vec::with_capacity(4);
    if super::sms(channels) {
        fields.extend(["phone", "phone-allowed"]);
    }
    if super::email(channels) {
        fields.extend(["email", "email-allowed"]);
    }
    fields
}

fn apply_user_mapping(
    mut users: Users,
    id: &str,
    name: Option<&str>,
    phone: Option<(String, bool)>,
    email: Option<(String, bool)>,
    response_email: Option<&str>,
) -> Result<Users> {
    let id = UserId::parse(id)?;
    if users.user(&id).is_none() {
        let name = name
            .filter(|name| !name.trim().is_empty())
            .context("a new receiver user requires --user-name")?;
        users.users.push(User {
            id: id.clone(),
            name: name.trim().to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        });
    }
    let user = users.user_mut(&id).expect("user was selected or inserted");
    if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
        name.trim().clone_into(&mut user.name);
    }
    if let Some((value, inbound_allowed)) = phone {
        let value = crate::users::normalize_phone(&value)
            .map_err(|_| anyhow::anyhow!("phone address is invalid"))?;
        upsert_phone(&mut user.phones, value, inbound_allowed);
    }
    if let Some((value, inbound_allowed)) = email {
        let value = crate::users::normalize_email(&value)
            .map_err(|_| anyhow::anyhow!("email address is invalid"))?;
        upsert_email(&mut user.emails, value, inbound_allowed);
    }
    if let Some(response) = response_email {
        let response = crate::users::normalize_email(response)
            .map_err(|_| anyhow::anyhow!("response email address is invalid"))?;
        if !user.emails.iter().any(|email| email.value == response) {
            user.emails.push(EmailIdentity {
                value: response.clone(),
                inbound_allowed: false,
            });
        }
        user.response_email = Some(response);
    }
    users
        .to_bytes()
        .map_err(|_| anyhow::anyhow!("receiver user mapping conflicts with portable identities"))?;
    Ok(users)
}

fn upsert_phone(phones: &mut Vec<PhoneIdentity>, value: String, inbound_allowed: bool) {
    if let Some(phone) = phones.iter_mut().find(|phone| phone.value == value) {
        phone.inbound_allowed = inbound_allowed;
    } else {
        phones.push(PhoneIdentity {
            value,
            inbound_allowed,
        });
    }
}

fn upsert_email(emails: &mut Vec<EmailIdentity>, value: String, inbound_allowed: bool) {
    if let Some(email) = emails.iter_mut().find(|email| email.value == value) {
        email.inbound_allowed = inbound_allowed;
    } else {
        emails.push(EmailIdentity {
            value,
            inbound_allowed,
        });
    }
}

fn load_or_empty(workspace: &WorkspaceContext) -> Result<Users> {
    match UsersStore::load(workspace) {
        Ok(users) => Ok(users),
        Err(error) if error.is_missing_store() => Ok(Users::empty()),
        Err(error) => Err(error.into()),
    }
}

fn prompt(label: &str) -> Result<String> {
    let prompt = format!("{} ", crate::theme::Theme::active().prompt(label));
    let value = crate::command::configuration::prompt_tty_line(&prompt)?
        .context("receiver setup cancelled before portable users changed")?;
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "{label} cannot be empty");
    Ok(value.to_owned())
}

fn prompt_allowed(label: &str) -> Result<bool> {
    let value = crate::command::configuration::prompt_tty_line(&format!(
        "{} ",
        crate::theme::Theme::active().prompt(label)
    ))?
    .context("receiver setup cancelled before portable users changed")?;
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "y" | "yes" | "true" => Ok(true),
        "n" | "no" | "false" => Ok(false),
        _ => anyhow::bail!("answer yes or no"),
    }
}

#[cfg(test)]
mod tests;
