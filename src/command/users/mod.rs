//! Selected-workspace portable-user command handling.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

use crate::cli::{UserAction, UserArgs};
use crate::theme::Theme;
use crate::users::{
    EmailIdentity, PhoneIdentity, User, UserId, UserMutation, Users, UsersStore, apply_mutation,
    normalize_email, normalize_phone,
};
use crate::workspace::{RegistryStore, WorkspaceContext, WorkspaceManifest};

mod reassign;
mod removal;
mod select;

use removal::remove_user;
use select::{Choice, interpret_row, numbered_rows};

pub fn run(args: &UserArgs, selector: Option<&str>, store: &RegistryStore) -> Result<()> {
    if std::io::stdin().is_terminal() && needs_prompt(&args.action) {
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("open /dev/tty for user prompts")?;
        let mut writer = tty.try_clone().context("clone user prompt terminal")?;
        let mut reader = BufReader::new(tty);
        return run_with_io(
            args,
            selector,
            store,
            &mut reader,
            &mut writer,
            Theme::active(),
        );
    }
    run_with_io(
        args,
        selector,
        store,
        &mut std::io::empty(),
        &mut std::io::sink(),
        Theme::active(),
    )
}

fn run_with_io(
    args: &UserArgs,
    selector: Option<&str>,
    store: &RegistryStore,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    theme: Theme,
) -> Result<()> {
    let workspace = selected_workspace(selector, store)?;
    match &args.action {
        UserAction::List => {
            print_users(&UsersStore::load(&workspace)?, theme);
            Ok(())
        }
        UserAction::Add {
            id,
            name,
            phone,
            email,
            response_email,
        } => {
            let id = user_id(id.as_deref(), reader, writer, theme)?;
            let name = required_value(name.as_deref(), "Display name:", reader, writer, theme)?;
            let mut emails = normalized_emails(&id, email, true)?;
            let response_email = normalized_response(&id, response_email.as_deref())?;
            if let Some(response) = response_email.as_deref()
                && !emails.iter().any(|email| email.value == response)
            {
                emails.push(EmailIdentity {
                    value: response.to_owned(),
                    inbound_allowed: false,
                });
            }
            let user = User {
                id,
                name,
                phones: normalized_phones(phone, true)?,
                emails,
                response_email,
            };
            let mut users = load_or_empty(&workspace)?;
            apply_mutation(&mut users, UserMutation::Add(user))?;
            UsersStore::save(&workspace, &users)?;
            println!("{}", theme.success("Portable user added"));
            Ok(())
        }
        UserAction::Update {
            id,
            name,
            add_phone,
            add_email,
            response_email,
        } => {
            let id = user_id(id.as_deref(), reader, writer, theme)?;
            let prompt_name = name.is_none()
                && add_phone.is_empty()
                && add_email.is_empty()
                && response_email.is_none();
            let name = if prompt_name {
                Some(required_value(
                    None,
                    "New display name:",
                    reader,
                    writer,
                    theme,
                )?)
            } else {
                name.clone()
            };
            let mut users = UsersStore::load(&workspace)?;
            apply_mutation(
                &mut users,
                UserMutation::Update {
                    id,
                    name,
                    add_phones: add_phone.clone(),
                    add_emails: add_email.clone(),
                    response_email: response_email.clone(),
                },
            )?;
            UsersStore::save(&workspace, &users)?;
            println!("{}", theme.success("Portable user updated"));
            Ok(())
        }
        UserAction::Reassign { from, to } => {
            let users = UsersStore::load(&workspace)?;
            let from = match from {
                Some(value) => {
                    required_value(Some(value), "Assignment value:", reader, writer, theme)?
                }
                None => prompt_assignment_value(&workspace, &users, reader, writer, theme)?,
            };
            let to = match to {
                Some(value) => UserId::parse(value)?,
                None => prompt_member(&users, reader, writer, theme)?,
            };
            let moved = reassign::reassign(&workspace, &from, &to)?;
            let summary = moved_summary(moved, &from, &to);
            if moved == 0 {
                println!("{}", theme.warning(&summary));
            } else {
                println!("{}", theme.success(&summary));
            }
            Ok(())
        }
        UserAction::Remove { id, reassign_to } => {
            let id = user_id(id.as_deref(), reader, writer, theme)?;
            let replacement = reassign_to
                .as_deref()
                .map(UserId::parse)
                .transpose()
                .map_err(anyhow::Error::from)?;
            remove_user(&workspace, &id, replacement.as_ref())?;
            println!("{}", theme.success("Portable user removed"));
            Ok(())
        }
        UserAction::Local { id } => {
            let id = user_id(id.as_deref(), reader, writer, theme)?;
            let users = UsersStore::load(&workspace)?;
            if users.user(&id).is_none() {
                return Err(crate::users::UsersError::UnknownUser {
                    user_id: id.to_string(),
                }
                .into());
            }
            set_local_user(store, &workspace, &id)?;
            println!("{}", theme.success("Local user selected"));
            Ok(())
        }
    }
}

fn selected_workspace(selector: Option<&str>, store: &RegistryStore) -> Result<WorkspaceContext> {
    let registry = RegistryStore::load_from(store.path())?;
    let selected = registry.select(selector)?;
    if !selected.record().root.is_dir() {
        anyhow::bail!(
            "workspace root {} is unavailable",
            selected.record().root.display()
        );
    }
    let manifest = WorkspaceManifest::load(&selected.record().root, env!("CARGO_PKG_VERSION"))?;
    if manifest.workspace_id() != selected.record().workspace_id {
        anyhow::bail!("workspace manifest UUID does not match the selected registry record");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))?;
    let current_dir = std::env::current_dir().context("read current directory")?;
    Ok(WorkspaceContext::new(
        &home,
        selected.record().workspace_id,
        selected.canonical_name().clone(),
        &selected.record().root,
        selected.record().local_user_id.clone(),
        &current_dir,
    )?)
}

fn load_or_empty(workspace: &WorkspaceContext) -> Result<Users> {
    match UsersStore::load(workspace) {
        Ok(users) => Ok(users),
        Err(error) if error.is_missing_store() => Ok(Users::empty()),
        Err(error) => Err(error.into()),
    }
}

fn print_users(users: &Users, theme: Theme) {
    println!("{}", theme.heading("Portable users"));
    for user in &users.users {
        println!(
            "{}  {}",
            theme.accent(user.id.as_str()),
            theme.value(&user.name)
        );
        for phone in &user.phones {
            println!(
                "  {} {}{}",
                theme.muted("phone"),
                phone.value,
                if phone.inbound_allowed {
                    " (inbound)"
                } else {
                    ""
                }
            );
        }
        for email in &user.emails {
            println!(
                "  {} {}{}",
                theme.muted("email"),
                email.value,
                if email.inbound_allowed {
                    " (inbound)"
                } else {
                    ""
                }
            );
        }
        if let Some(response) = user.response_email.as_deref() {
            println!("  {} {response}", theme.muted("response"));
        }
    }
}

fn set_local_user(store: &RegistryStore, workspace: &WorkspaceContext, id: &UserId) -> Result<()> {
    store.transaction(|transaction| -> Result<()> {
        let mut registry = transaction.load()?;
        let selected = registry.select(Some(workspace.name().as_str()))?;
        if selected.record().workspace_id != workspace.id() {
            anyhow::bail!("selected workspace identity changed during local-user update");
        }
        let name = selected.canonical_name().clone();
        transaction.update(&mut registry, |candidate| {
            candidate.set_local_user(&name, id)
        })?;
        Ok(())
    })
}

fn moved_summary(moved: usize, from: &str, to: &UserId) -> String {
    match moved {
        0 => format!("No task or habit is assigned to {from}"),
        1 => format!("Moved 1 task from {from} to {to}"),
        _ => format!("Moved {moved} tasks from {from} to {to}"),
    }
}

fn prompt_assignment_value(
    workspace: &WorkspaceContext,
    users: &Users,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    theme: Theme,
) -> Result<String> {
    let values = reassign::assignment_values(workspace)?;
    let choices = reassign::unmapped_assignments(&values, users)
        .iter()
        .map(|value| Choice::new(value, value))
        .collect::<Vec<_>>();
    if !choices.is_empty() {
        writeln!(
            writer,
            "{}",
            theme.heading("Assignment values with no portable person")
        )?;
    }
    select_value("Move work assigned to:", &choices, reader, writer, theme)
}

fn prompt_member(
    users: &Users,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    theme: Theme,
) -> Result<UserId> {
    let choices = users
        .users
        .iter()
        .map(|user| Choice::new(user.id.as_str(), &format!("{} ({})", user.id, user.name)))
        .collect::<Vec<_>>();
    writeln!(writer, "{}", theme.heading("Portable users"))?;
    let value = select_value("Move that work to:", &choices, reader, writer, theme)?;
    UserId::parse(&value).map_err(Into::into)
}

fn select_value(
    label: &str,
    choices: &[Choice],
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    theme: Theme,
) -> Result<String> {
    for row in numbered_rows(choices) {
        writeln!(writer, "  {}", theme.value(&row))?;
    }
    loop {
        let answer = required_value(None, label, reader, writer, theme)?;
        if let Some(value) = interpret_row(choices, &answer) {
            return Ok(value);
        }
        writeln!(
            writer,
            "{}",
            theme.warning("Answer with one of the numbers above or an exact value.")
        )?;
    }
}

fn needs_prompt(action: &UserAction) -> bool {
    match action {
        UserAction::List => false,
        UserAction::Add { id, name, .. } => id.is_none() || name.is_none(),
        UserAction::Update {
            id,
            name,
            add_phone,
            add_email,
            response_email,
        } => {
            id.is_none()
                || (name.is_none()
                    && add_phone.is_empty()
                    && add_email.is_empty()
                    && response_email.is_none())
        }
        UserAction::Reassign { from, to } => from.is_none() || to.is_none(),
        UserAction::Remove { id, .. } | UserAction::Local { id } => id.is_none(),
    }
}

fn user_id(
    value: Option<&str>,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    theme: Theme,
) -> Result<UserId> {
    let value = required_value(value, "User ID:", reader, writer, theme)?;
    UserId::parse(&value).map_err(Into::into)
}

fn required_value(
    value: Option<&str>,
    label: &str,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    theme: Theme,
) -> Result<String> {
    if let Some(value) = value {
        if value.trim().is_empty() {
            anyhow::bail!("{label} cannot be empty");
        }
        return Ok(value.trim().to_owned());
    }
    loop {
        write!(writer, "{} ", theme.prompt(label)).context("write user prompt")?;
        writer.flush().context("flush user prompt")?;
        let mut line = String::new();
        if reader.read_line(&mut line).context("read user prompt")? == 0 {
            anyhow::bail!("user command cancelled before portable data changed");
        }
        if !line.trim().is_empty() {
            return Ok(line.trim().to_owned());
        }
        writeln!(writer, "{}", theme.warning("A value is required."))?;
    }
}

fn normalized_phones(values: &[String], inbound_allowed: bool) -> Result<Vec<PhoneIdentity>> {
    values
        .iter()
        .map(|value| {
            normalize_phone(value)
                .map(|value| PhoneIdentity {
                    value,
                    inbound_allowed,
                })
                .map_err(|_| anyhow!("phone `{value}` is invalid"))
        })
        .collect()
}

fn normalized_emails(
    id: &UserId,
    values: &[String],
    inbound_allowed: bool,
) -> Result<Vec<EmailIdentity>> {
    values
        .iter()
        .map(|value| {
            normalize_email(value)
                .map(|value| EmailIdentity {
                    value,
                    inbound_allowed,
                })
                .map_err(|_| {
                    crate::users::UsersError::InvalidEmail {
                        user_id: id.to_string(),
                        value: value.clone(),
                    }
                    .into()
                })
        })
        .collect()
}

fn normalized_response(id: &UserId, value: Option<&str>) -> Result<Option<String>> {
    value
        .map(|value| {
            normalize_email(value).map_err(|_| {
                crate::users::UsersError::InvalidEmail {
                    user_id: id.to_string(),
                    value: value.to_owned(),
                }
                .into()
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::moved_summary;
    use crate::users::UserId;

    #[test]
    fn the_move_summary_counts_tasks_in_plain_singular_and_plural_english() {
        let to = UserId::parse("pablo").unwrap();

        assert_eq!(
            moved_summary(0, "ghost", &to),
            "No task or habit is assigned to ghost"
        );
        assert_eq!(moved_summary(1, "me", &to), "Moved 1 task from me to pablo");
        assert_eq!(
            moved_summary(4, "me", &to),
            "Moved 4 tasks from me to pablo"
        );
    }
}
