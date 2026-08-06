use super::*;

pub(super) fn moved_summary(moved: usize, from: &str, to: &UserId) -> String {
    match moved {
        0 => format!("No task or habit is assigned to {from}"),
        1 => format!("Moved 1 task from {from} to {to}"),
        _ => format!("Moved {moved} tasks from {from} to {to}"),
    }
}

pub(super) fn prompt_assignment_value(
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

pub(super) fn prompt_member(
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

pub(super) fn select_value(
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

pub(super) fn needs_prompt(action: &UserAction) -> bool {
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

pub(super) fn user_id(
    value: Option<&str>,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    theme: Theme,
) -> Result<UserId> {
    let value = required_value(value, "User ID:", reader, writer, theme)?;
    UserId::parse(&value).map_err(Into::into)
}

pub(super) fn required_value(
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

pub(super) fn normalized_phones(
    values: &[String],
    inbound_allowed: bool,
) -> Result<Vec<PhoneIdentity>> {
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

pub(super) fn normalized_emails(
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

pub(super) fn normalized_response(id: &UserId, value: Option<&str>) -> Result<Option<String>> {
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
