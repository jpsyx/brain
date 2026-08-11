//! The config variables that portable `users.json` superseded.
//!
//! Three declared variables are legacy migration input in the config store, but
//! the answer brain actually enforces lives in the portable users roster.
//! Resolving them from the config store alone made a fully configured receiver
//! report `(unset)`, which reads as "setup did nothing" — so these commands
//! resolve the live roster first and say which store owns it.

use crate::users::Users;

/// The declared config variables whose live value belongs to portable users.
pub(crate) const SUPERSEDED_BY_USERS: [&str; 3] = [
    "response_email",
    "allowed_sms_senders",
    "allowed_email_senders",
];

/// How the roster's entries are joined into one config-shaped value.
const SEPARATOR: &str = ", ";

/// Whether the portable roster owns `name`'s live value. Pure.
#[must_use]
pub(crate) fn is_superseded(name: &str) -> bool {
    SUPERSEDED_BY_USERS.contains(&name)
}

/// One superseded variable's live value from the portable roster. Pure.
///
/// `None` means no portable user answers for it, which is the only case where
/// the config store's own legacy value is still the best answer.
#[must_use]
pub(crate) fn active_value(name: &str, users: &Users) -> Option<String> {
    let values: Vec<String> = match name {
        "allowed_sms_senders" => users
            .users
            .iter()
            .flat_map(|user| user.phones.iter())
            .filter(|identity| identity.inbound_allowed)
            .map(|identity| identity.value.clone())
            .collect(),
        "allowed_email_senders" => users
            .users
            .iter()
            .flat_map(|user| user.emails.iter())
            .filter(|identity| identity.inbound_allowed)
            .map(|identity| identity.value.clone())
            .collect(),
        "response_email" => users
            .users
            .iter()
            .filter_map(|user| user.response_email.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => return None,
    };
    join_unique(values)
}

/// Roster order, first occurrence wins, `None` when nothing qualifies. Pure.
fn join_unique(values: Vec<String>) -> Option<String> {
    let mut seen = Vec::new();
    for value in values {
        if !seen.contains(&value) {
            seen.push(value);
        }
    }
    (!seen.is_empty()).then(|| seen.join(SEPARATOR))
}

/// The muted note `config list` prints under the table. Pure.
#[must_use]
pub(crate) fn source_note(workspace: &str) -> String {
    format!(
        "{} are portable: their live values come from users.json, not this store.\nInspect them with `brain user list -w {workspace}` and change them with `brain user`.",
        SUPERSEDED_BY_USERS.join(", ")
    )
}

/// Why `config set` refuses a superseded variable, and what to run instead. Pure.
#[must_use]
pub(crate) fn refusal(name: &str, workspace: &str) -> String {
    format!(
        "{name} is portable: its live value lives in users.json, and writing it here would persist a value nothing reads.\n  \
         inspect: brain user list -w {workspace}\n  \
         change:  brain user add -w {workspace} --id <USER_ID> --name <DISPLAY_NAME> --phone <PHONE> --email <EMAIL>\n  \
         or:      brain receiver setup -w {workspace}"
    )
}

#[cfg(test)]
mod tests {
    use super::{SUPERSEDED_BY_USERS, active_value, is_superseded, refusal, source_note};
    use crate::users::{EmailIdentity, PhoneIdentity, USERS_SCHEMA_VERSION, User, UserId, Users};

    fn user(
        id: &str,
        phones: &[(&str, bool)],
        emails: &[(&str, bool)],
        response_email: Option<&str>,
    ) -> User {
        User {
            id: UserId::parse(id).expect("user id"),
            name: id.to_owned(),
            phones: phones
                .iter()
                .map(|(value, inbound_allowed)| PhoneIdentity {
                    value: (*value).to_owned(),
                    inbound_allowed: *inbound_allowed,
                })
                .collect(),
            emails: emails
                .iter()
                .map(|(value, inbound_allowed)| EmailIdentity {
                    value: (*value).to_owned(),
                    inbound_allowed: *inbound_allowed,
                })
                .collect(),
            response_email: response_email.map(str::to_owned),
        }
    }

    fn roster(users: Vec<User>) -> Users {
        Users {
            schema_version: USERS_SCHEMA_VERSION,
            users,
        }
    }

    #[test]
    fn exactly_the_three_receiver_identity_variables_are_superseded() {
        for name in SUPERSEDED_BY_USERS {
            assert!(is_superseded(name), "{name}");
        }
        for name in ["access_mode", "agenda_dir", "calendar_id"] {
            assert!(!is_superseded(name), "{name}");
        }
    }

    #[test]
    fn an_allow_list_collects_every_inbound_identity_across_the_roster() {
        let users = roster(vec![
            user(
                "pablo",
                &[("+12125550100", true)],
                &[("a@x.test", true)],
                None,
            ),
            user(
                "sam",
                &[("+12125550199", true)],
                &[("b@x.test", true)],
                None,
            ),
        ]);

        assert_eq!(
            active_value("allowed_sms_senders", &users).as_deref(),
            Some("+12125550100, +12125550199")
        );
        assert_eq!(
            active_value("allowed_email_senders", &users).as_deref(),
            Some("a@x.test, b@x.test")
        );
    }

    #[test]
    fn an_identity_that_may_not_initiate_work_is_not_an_allowed_sender() {
        // `inbound_allowed` is the whole authorization decision; a listed but
        // disallowed number reported as allowed would be a security lie.
        let users = roster(vec![user(
            "pablo",
            &[("+12125550100", false), ("+12125550199", true)],
            &[("a@x.test", false)],
            None,
        )]);

        assert_eq!(
            active_value("allowed_sms_senders", &users).as_deref(),
            Some("+12125550199")
        );
        assert_eq!(active_value("allowed_email_senders", &users), None);
    }

    #[test]
    fn one_identity_shared_by_two_people_is_listed_once() {
        let users = roster(vec![
            user("pablo", &[("+12125550100", true)], &[], None),
            user("sam", &[("+12125550100", true)], &[], None),
        ]);

        assert_eq!(
            active_value("allowed_sms_senders", &users).as_deref(),
            Some("+12125550100")
        );
    }

    #[test]
    fn a_response_address_is_reported_per_person_and_blanks_are_ignored() {
        let users = roster(vec![
            user("pablo", &[], &[], Some("long@x.test")),
            user("sam", &[], &[], Some("   ")),
            user("kim", &[], &[], None),
        ]);

        assert_eq!(
            active_value("response_email", &users).as_deref(),
            Some("long@x.test")
        );
    }

    #[test]
    fn an_empty_roster_leaves_the_config_stores_own_value_as_the_best_answer() {
        let users = roster(vec![user("pablo", &[], &[], None)]);

        for name in SUPERSEDED_BY_USERS {
            assert_eq!(active_value(name, &users), None, "{name}");
        }
    }

    #[test]
    fn the_note_and_the_refusal_both_name_the_store_and_the_working_command() {
        let note = source_note("family");
        assert!(note.contains("users.json"), "{note}");
        assert!(note.contains("brain user list -w family"), "{note}");

        let refusal = refusal("allowed_sms_senders", "family");
        assert!(refusal.contains("allowed_sms_senders"), "{refusal}");
        assert!(refusal.contains("users.json"), "{refusal}");
        assert!(refusal.contains("brain user"), "{refusal}");
    }
}
