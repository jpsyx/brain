//! Pure rollout gates and focused step adapters.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

use crate::config::Config;
use crate::users::{
    AssignmentRewrites, User, UserId, UserMutation, Users, UsersError, apply_mutation,
};

/// A portable identity that must be mapped before rollout mutation starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingIssue {
    Phone(String),
    Email(String),
    Assignment(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingResolution {
    Existing(UserId),
    New { id: UserId, name: String },
}

/// Apply one interactive identity decision to an in-memory portable registry.
///
/// An assignment that adopts a portable member under another ID records a task
/// rewrite instead of creating a second person for the same human.
pub fn apply_mapping_resolution(
    users: &mut Users,
    rewrites: &mut AssignmentRewrites,
    issue: &MappingIssue,
    resolution: MappingResolution,
) -> Result<()> {
    let target = match resolution {
        MappingResolution::Existing(id) => {
            if users.user(&id).is_none() {
                return Err(UsersError::UnknownUser {
                    user_id: id.to_string(),
                }
                .into());
            }
            id
        }
        MappingResolution::New { id, name } => {
            apply_mutation(
                users,
                UserMutation::Add(User {
                    id: id.clone(),
                    name,
                    phones: Vec::new(),
                    emails: Vec::new(),
                    response_email: None,
                }),
            )?;
            id
        }
    };
    match issue {
        MappingIssue::Phone(value) => apply_mutation(
            users,
            UserMutation::Update {
                id: target,
                name: None,
                add_phones: vec![value.clone()],
                add_emails: Vec::new(),
                response_email: None,
            },
        )?,
        MappingIssue::Email(value) => apply_mutation(
            users,
            UserMutation::Update {
                id: target,
                name: None,
                add_phones: Vec::new(),
                add_emails: vec![value.clone()],
                response_email: None,
            },
        )?,
        MappingIssue::Assignment(value) => {
            if value.trim() != target.as_str() {
                rewrites.record(value, &target);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationGateInput {
    pub sync_configured: bool,
    pub interactive: bool,
    pub explicit_workspace: bool,
    pub acknowledged_all_machines_updated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationGate {
    Proceed,
    ConfirmAllMachinesUpdated,
}

/// Decide whether the legacy-sync compatibility gate is satisfied.
pub fn migration_gate(input: MigrationGateInput) -> Result<MigrationGate> {
    if !input.sync_configured {
        return Ok(MigrationGate::Proceed);
    }
    if input.interactive {
        return Ok(MigrationGate::ConfirmAllMachinesUpdated);
    }
    if !input.explicit_workspace || !input.acknowledged_all_machines_updated {
        bail!(
            "headless migration of a synced workspace requires `brain workspace migrate --workspace <WORKSPACE> --acknowledge-all-machines-updated`"
        );
    }
    Ok(MigrationGate::Proceed)
}

/// Find every legacy sender and assignment not represented by portable users.
#[must_use]
pub fn mapping_issues(users: &Users, config: &Config, assignments: &[String]) -> Vec<MappingIssue> {
    let mut issues = Vec::new();
    let mut seen_phones = BTreeSet::new();
    for phone in config.allowed_sms() {
        if seen_phones.insert(phone.clone()) && users.resolve_phone(&phone).is_none() {
            issues.push(MappingIssue::Phone(phone));
        }
    }
    let mut seen_emails = BTreeSet::new();
    for email in config.allowed_email() {
        if seen_emails.insert(email.clone()) && users.resolve_email(&email).is_none() {
            issues.push(MappingIssue::Email(email));
        }
    }
    let mut seen = BTreeSet::new();
    for assignment in assignments {
        let assignment = assignment.trim();
        if assignment.is_empty() || !seen.insert(assignment.to_owned()) {
            continue;
        }
        let mapped = UserId::parse(assignment)
            .ok()
            .is_some_and(|id| users.user(&id).is_some());
        if !mapped {
            issues.push(MappingIssue::Assignment(assignment.to_owned()));
        }
    }
    issues
}

/// Render exact non-interactive commands for every unresolved portable identity.
#[must_use]
pub fn headless_mapping_remediation(workspace: &str, issues: &[MappingIssue]) -> String {
    issues
        .iter()
        .map(|issue| match issue {
            MappingIssue::Phone(value) => {
                format!("brain user update <USER_ID> -w {workspace} --add-phone {value}")
            }
            MappingIssue::Email(value) => {
                format!("brain user update <USER_ID> -w {workspace} --add-email {value}")
            }
            MappingIssue::Assignment(value) => format!(
                "brain user add -w {workspace} --id {value} --name <DISPLAY_NAME>\nbrain user reassign {value} <EXISTING_USER_ID> -w {workspace}"
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
