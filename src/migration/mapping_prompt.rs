//! Pure text and answer interpretation for the interactive identity mapping.

use crate::users::{UserId, Users};

use super::MappingIssue;

/// What one answer to the identity-mapping prompt means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MappingChoice {
    /// Attach the legacy identity to a portable member who already exists.
    Existing(UserId),
    /// Add a person who is not in the portable registry yet.
    CreateNew,
}

/// Ask, in plain English, who one unmapped legacy identity belongs to.
pub(super) fn mapping_question(issue: &MappingIssue) -> String {
    match issue {
        MappingIssue::Phone(value) => format!("Which person sends messages from {value}?"),
        MappingIssue::Email(value) => format!("Which person sends email from {value}?"),
        MappingIssue::Assignment(value) => {
            format!("Tasks are assigned to \"{value}\". Which person is that?")
        }
    }
}

/// Render every numbered answer, existing members first.
pub(super) fn mapping_options(issue: &MappingIssue, users: &Users) -> Vec<String> {
    let mut options = users
        .users
        .iter()
        .enumerate()
        .map(|(index, user)| format!("{}) {} ({})", index + 1, user.id, user.name))
        .collect::<Vec<_>>();
    let create = proposed_new_id(issue).map_or_else(
        || "Add a new person".to_owned(),
        |id| format!("Add a new person with ID \"{id}\""),
    );
    options.push(format!("{}) {create}", users.users.len() + 1));
    options
}

/// Interpret one answer as a row number or a typed portable user ID.
pub(super) fn interpret_choice(users: &Users, answer: &str) -> Option<MappingChoice> {
    let answer = answer.trim();
    if let Ok(row) = answer.parse::<usize>() {
        if row >= 1 && row <= users.users.len() {
            return Some(MappingChoice::Existing(users.users[row - 1].id.clone()));
        }
        return (row == users.users.len() + 1).then_some(MappingChoice::CreateNew);
    }
    let id = UserId::parse(answer).ok()?;
    users
        .user(&id)
        .map(|user| MappingChoice::Existing(user.id.clone()))
}

/// The portable user ID a new person would keep from the legacy identity.
pub(super) fn proposed_new_id(issue: &MappingIssue) -> Option<UserId> {
    match issue {
        MappingIssue::Assignment(value) => UserId::parse(value.trim()).ok(),
        MappingIssue::Phone(_) | MappingIssue::Email(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{MappingChoice, interpret_choice, mapping_options, mapping_question};
    use crate::migration::MappingIssue;
    use crate::users::{User, UserId, Users};

    fn users() -> Users {
        Users {
            schema_version: 1,
            users: vec![
                User {
                    id: UserId::parse("pablo").unwrap(),
                    name: "Pablo".to_owned(),
                    phones: Vec::new(),
                    emails: Vec::new(),
                    response_email: None,
                },
                User {
                    id: UserId::parse("alex").unwrap(),
                    name: "Alex".to_owned(),
                    phones: Vec::new(),
                    emails: Vec::new(),
                    response_email: None,
                },
            ],
        }
    }

    #[test]
    fn every_question_names_its_legacy_identity_and_reads_as_a_sentence() {
        assert_eq!(
            mapping_question(&MappingIssue::Assignment("me".to_owned())),
            "Tasks are assigned to \"me\". Which person is that?"
        );
        assert_eq!(
            mapping_question(&MappingIssue::Phone("+12125550100".to_owned())),
            "Which person sends messages from +12125550100?"
        );
        assert_eq!(
            mapping_question(&MappingIssue::Email("pablo@example.test".to_owned())),
            "Which person sends email from pablo@example.test?"
        );
    }

    #[test]
    fn existing_members_are_offered_before_adding_anyone_new() {
        assert_eq!(
            mapping_options(&MappingIssue::Assignment("me".to_owned()), &users()),
            [
                "1) pablo (Pablo)",
                "2) alex (Alex)",
                "3) Add a new person with ID \"me\"",
            ]
        );
        assert_eq!(
            mapping_options(&MappingIssue::Assignment("Pablo S".to_owned()), &users())
                .last()
                .unwrap(),
            "3) Add a new person",
            "a legacy value that is not a valid ID cannot become one"
        );
        assert_eq!(
            mapping_options(&MappingIssue::Phone("+12125550100".to_owned()), &users())
                .last()
                .unwrap(),
            "3) Add a new person"
        );
    }

    #[test]
    fn an_answer_selects_by_row_number_or_by_typed_member_id() {
        let users = users();

        assert_eq!(
            interpret_choice(&users, " 2 "),
            Some(MappingChoice::Existing(UserId::parse("alex").unwrap()))
        );
        assert_eq!(
            interpret_choice(&users, "pablo"),
            Some(MappingChoice::Existing(UserId::parse("pablo").unwrap()))
        );
        assert_eq!(
            interpret_choice(&users, "3"),
            Some(MappingChoice::CreateNew)
        );
        assert_eq!(interpret_choice(&users, "0"), None);
        assert_eq!(interpret_choice(&users, "4"), None);
        assert_eq!(interpret_choice(&users, "nobody"), None);
        assert_eq!(interpret_choice(&users, ""), None);
    }
}
