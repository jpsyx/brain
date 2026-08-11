//! Numbered option prompts for choosing a portable user.
//!
//! Shared by the `brain user` commands and by readiness repair, which has to
//! ask which member this machine is before any ordinary command can run.

/// One offered answer: the value it produces and the row a human reads.
pub(crate) struct Choice {
    pub(crate) value: String,
    pub(crate) label: String,
}

impl Choice {
    pub(crate) fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_owned(),
            label: label.to_owned(),
        }
    }
}

/// Render one numbered option list, ready to print one row per line.
pub(crate) fn numbered_rows(choices: &[Choice]) -> Vec<String> {
    choices
        .iter()
        .enumerate()
        .map(|(index, choice)| format!("{}) {}", index + 1, choice.label))
        .collect()
}

/// Interpret one answer as a row number, falling back to the literal value.
pub(crate) fn interpret_row(choices: &[Choice], answer: &str) -> Option<String> {
    let answer = answer.trim();
    if answer.is_empty() {
        return None;
    }
    if let Ok(row) = answer.parse::<usize>() {
        return choices
            .get(row.checked_sub(1)?)
            .map(|choice| choice.value.clone());
    }
    Some(answer.to_owned())
}

/// The offered members when a machine must choose its local person. Pure.
///
/// The value is the portable user ID the registry stores; the label is what a
/// human recognizes, since nobody should be expected to recall an ID they never
/// typed.
#[must_use]
pub(crate) fn local_user_choices(users: &super::Users) -> Vec<Choice> {
    users
        .users
        .iter()
        .map(|user| Choice::new(user.id.as_str(), &format!("{} ({})", user.id, user.name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Choice, interpret_row, numbered_rows};

    fn choices() -> Vec<Choice> {
        vec![
            Choice::new("pablo", "pablo (Pablo)"),
            Choice::new("wife", "wife (Wife)"),
        ]
    }

    #[test]
    fn options_are_numbered_from_one() {
        assert_eq!(
            numbered_rows(&choices()),
            ["1) pablo (Pablo)", "2) wife (Wife)"]
        );
        assert!(numbered_rows(&[]).is_empty());
    }

    #[test]
    fn the_local_user_options_pair_every_id_with_the_name_a_human_recognizes() {
        let users = crate::users::Users {
            schema_version: crate::users::USERS_SCHEMA_VERSION,
            users: vec![
                crate::users::User {
                    id: crate::users::UserId::parse("pablo").unwrap(),
                    name: "Pablo".to_owned(),
                    phones: Vec::new(),
                    emails: Vec::new(),
                    response_email: None,
                },
                crate::users::User {
                    id: crate::users::UserId::parse("sun").unwrap(),
                    name: "Sun".to_owned(),
                    phones: Vec::new(),
                    emails: Vec::new(),
                    response_email: None,
                },
            ],
        };

        let choices = super::local_user_choices(&users);

        assert_eq!(
            numbered_rows(&choices),
            ["1) pablo (Pablo)", "2) sun (Sun)"]
        );
        // The value is the stored ID, never the label a human read.
        assert_eq!(interpret_row(&choices, "2"), Some("sun".to_owned()));
    }

    #[test]
    fn an_answer_is_a_row_number_or_the_literal_value_itself() {
        let choices = choices();

        assert_eq!(interpret_row(&choices, " 2 "), Some("wife".to_owned()));
        assert_eq!(interpret_row(&choices, "me"), Some("me".to_owned()));
        assert_eq!(interpret_row(&choices, "0"), None);
        assert_eq!(interpret_row(&choices, "3"), None);
        assert_eq!(interpret_row(&choices, "  "), None);
    }
}
