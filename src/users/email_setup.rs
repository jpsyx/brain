//! Shared email selection for user creation and setup.

use anyhow::Result;

use super::{Choice, EmailIdentity, User, interpret_row, normalize_email, numbered_rows};

pub(crate) fn complete_email_configuration(
    user: &mut User,
    ask: &mut impl FnMut(&str) -> Result<String>,
) -> Result<()> {
    while !user.emails.iter().any(|email| email.inbound_allowed) {
        let answer = ask("Inbound emails (user -> brain), comma-separated:")?;
        let values = answer
            .split(',')
            .map(normalize_email)
            .collect::<Result<Vec<_>, _>>();
        let Ok(values) = values else { continue };
        for value in values {
            if let Some(email) = user.emails.iter_mut().find(|email| email.value == value) {
                email.inbound_allowed = true;
            } else {
                user.emails.push(EmailIdentity {
                    value,
                    inbound_allowed: true,
                });
            }
        }
    }
    if user.validate_email_configuration().is_ok() {
        return Ok(());
    }
    let choices = user
        .emails
        .iter()
        .filter(|email| email.inbound_allowed)
        .map(|email| Choice::new(&email.value, &email.value))
        .collect::<Vec<_>>();
    let label = format!(
        "{}\nOutbound email (brain -> user), choose an inbound email by number or address:",
        numbered_rows(&choices).join("\n")
    );
    loop {
        let answer = ask(&label)?;
        if let Some(value) = interpret_row(&choices, &answer)
            && let Ok(value) = normalize_email(&value)
            && choices.iter().any(|choice| choice.value == value)
        {
            user.response_email = Some(value);
            return user.validate_email_configuration().map_err(Into::into);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_picker_rejects_unknown_and_disabled_addresses_then_normalizes_choice() {
        let mut user = User {
            id: super::super::UserId::parse("alex").unwrap(),
            name: "Alex".into(),
            phones: vec![],
            emails: vec![
                EmailIdentity {
                    value: "alex@example.com".into(),
                    inbound_allowed: true,
                },
                EmailIdentity {
                    value: "disabled@example.com".into(),
                    inbound_allowed: false,
                },
                EmailIdentity {
                    value: "work@example.com".into(),
                    inbound_allowed: true,
                },
            ],
            response_email: None,
        };
        let mut answers = [
            "other@example.com",
            "disabled@example.com",
            "3",
            "WORK@EXAMPLE.COM",
        ]
        .into_iter();
        complete_email_configuration(&mut user, &mut |label| {
            assert!(!label.contains("disabled@example.com"));
            Ok(answers.next().expect("picker asked too many times").into())
        })
        .unwrap();
        assert_eq!(user.response_email.as_deref(), Some("work@example.com"));
        assert!(answers.next().is_none());
    }

    #[test]
    fn inbound_collection_retries_invalid_input_and_deduplicates_before_outbound_selection() {
        let mut user = User {
            id: super::super::UserId::parse("alex").unwrap(),
            name: "Alex".into(),
            phones: vec![],
            emails: vec![],
            response_email: None,
        };
        let mut answers = [
            "invalid",
            "Alex@example.com, alex@example.com, work@example.com",
            "2",
        ]
        .into_iter();
        complete_email_configuration(&mut user, &mut |_| {
            Ok(answers.next().expect("unexpected prompt").into())
        })
        .unwrap();
        assert_eq!(user.emails.len(), 2);
        assert_eq!(user.response_email.as_deref(), Some("work@example.com"));
        assert!(user.validate_email_configuration().is_ok());
    }
}
