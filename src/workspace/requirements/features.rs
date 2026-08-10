use super::{FeatureStatus, PromptMetadata, Requirement, RequirementScope};
use crate::workspace::CommandContext;

pub(super) fn requirements(command: &CommandContext, name: &str) -> Vec<Requirement> {
    let config = crate::config::Config::try_load(&command.workspace);
    let mut rows = Vec::new();
    match config {
        Ok(config) => {
            rows.extend(super::capabilities::requirements(command, name, &config));
            let triage = triage_status(&command.workspace, &config);
            rows.push(Requirement::feature(
                RequirementScope::TriageHabits,
                triage.0,
                Vec::new(),
                format!("brain config set -w {name} enable_triage_habits=true"),
            ));
            rows.push(Requirement::feature(
                RequirementScope::TriageModal,
                triage.1,
                vec![PromptMetadata::plain("Daily triage name pattern")],
                format!("brain config set -w {name} daily_triage_name_pattern=<REGEX>"),
            ));
            rows.push(Requirement::feature(
                RequirementScope::Linear,
                linear_status(&config),
                vec![PromptMetadata::plain("Linear workspace slug")],
                format!("brain config set -w {name} linear_workspace=<SLUG>"),
            ));
        }
        Err(_) => {
            for scope in [
                RequirementScope::AccessPolicy,
                RequirementScope::TriageHabits,
                RequirementScope::TriageModal,
                RequirementScope::Linear,
            ] {
                rows.push(Requirement::feature(
                    scope,
                    FeatureStatus::Incomplete,
                    Vec::new(),
                    format!("brain config list -w {name}"),
                ));
            }
        }
    }
    rows.push(task_schema_requirement(command, name));
    rows.push(pdf_requirement(command));
    rows.extend(personalization_requirements(command, name));
    rows.push(Requirement::feature(
        RequirementScope::BrowserViews,
        FeatureStatus::Ready,
        Vec::new(),
        format!("brain -w {name}"),
    ));
    rows.push(Requirement::feature(
        RequirementScope::WebViews,
        FeatureStatus::Ready,
        Vec::new(),
        format!("brain server status -w {name}"),
    ));
    rows
}

fn triage_status(
    workspace: &crate::workspace::WorkspaceContext,
    config: &crate::config::Config,
) -> (FeatureStatus, FeatureStatus) {
    if !config.enable_triage_habits {
        return (FeatureStatus::Off, FeatureStatus::Off);
    }
    let habits = crate::tasks::task::load_habits(&workspace.root().join("tasks/habits.csv"));
    let habits_status = habits.map_or(FeatureStatus::Incomplete, |habits| {
        let daily = habits
            .iter()
            .any(|habit| habit.system_key == crate::tasks::triage_habits::DAILY_SYSTEM_KEY);
        let weekly = habits
            .iter()
            .any(|habit| habit.system_key == crate::tasks::triage_habits::WEEKLY_SYSTEM_KEY);
        if daily && weekly {
            FeatureStatus::Ready
        } else {
            FeatureStatus::Incomplete
        }
    });
    let pattern = config.daily_triage_name_pattern.trim();
    let modal_status = if pattern.is_empty() {
        FeatureStatus::Off
    } else if regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .is_ok()
    {
        FeatureStatus::Ready
    } else {
        FeatureStatus::Incomplete
    };
    (habits_status, modal_status)
}

/// Without `tasks/SCHEMA.json` every schema decision fails, so a workspace
/// missing it cannot sync. Reporting it keeps `sync status` from calling such a
/// workspace ready.
fn task_schema_requirement(command: &CommandContext, name: &str) -> Requirement {
    Requirement::feature(
        RequirementScope::TaskSchema,
        if crate::tasks::schema::document_present(command.workspace.root()) {
            FeatureStatus::Ready
        } else {
            FeatureStatus::Incomplete
        },
        Vec::new(),
        format!("brain -w {name}"),
    )
}

fn pdf_requirement(command: &CommandContext) -> Requirement {
    Requirement::feature(
        RequirementScope::PdfConversion,
        if crate::settings::configured_markdown_to_pdf_ready(command) {
            FeatureStatus::Ready
        } else {
            FeatureStatus::Incomplete
        },
        vec![PromptMetadata::plain("markdown-to-pdf executable")],
        "brain env set markdown_to_pdf_path=<EXECUTABLE_PATH>".to_owned(),
    )
}

fn personalization_requirements(command: &CommandContext, name: &str) -> Vec<Requirement> {
    let path = command
        .workspace
        .root()
        .join(".config/personalization.json");
    let local = command.workspace.local_user_id();
    let parsed = std::fs::read_to_string(&path).ok().map(|text| {
        // A store that is neither the keyed schema nor a legacy persona reads as
        // no personas at all, which is indistinguishable from "unset" here; use
        // the strict parse to tell a broken file from an absent one.
        serde_json::from_str::<serde_json::Value>(&text)
            .map(|_| crate::personalization::personas::Personas::parse(&text, local))
    });
    let statuses = match &parsed {
        None => [FeatureStatus::Off; 3],
        Some(Ok(personas)) => {
            let persona = personas.persona_of(local);
            [
                populated(&persona.role),
                populated(&persona.works_for),
                if persona.tag_styles.is_empty() {
                    FeatureStatus::Off
                } else {
                    FeatureStatus::Ready
                },
            ]
        }
        Some(Err(_)) => [FeatureStatus::Incomplete; 3],
    };
    let mut rows = [
        (
            RequirementScope::PersonalizationRole,
            statuses[0],
            "Role",
            format!("brain persona set -w {name} role=<ROLE>"),
        ),
        (
            RequirementScope::PersonalizationOrganization,
            statuses[1],
            "Organization",
            format!("brain persona set -w {name} works_for=<ORGANIZATION>"),
        ),
        (
            RequirementScope::PersonalizationTagStyles,
            statuses[2],
            "Tag styles",
            format!("brain config set -w {name} tags"),
        ),
    ]
    .into_iter()
    .map(|(scope, status, prompt, remediation)| {
        Requirement::feature(
            scope,
            status,
            vec![PromptMetadata::plain(prompt)],
            remediation,
        )
    })
    .collect::<Vec<_>>();
    rows.push(member_personas_requirement(command, name, parsed.as_ref()));
    rows
}

/// Other members' personas: reported, never prompted for.
///
/// Only the person at this machine is asked to fill one in (see
/// `personalization::onboarding`), so a teammate who has not personalized yet
/// surfaces here as an unmet optional feature rather than as a prompt on
/// somebody else's terminal.
fn member_personas_requirement(
    command: &CommandContext,
    name: &str,
    parsed: Option<&serde_json::Result<crate::personalization::personas::Personas>>,
) -> Requirement {
    let Some(Ok(personas)) = parsed else {
        return Requirement::feature(
            RequirementScope::MemberPersonas,
            if parsed.is_none() {
                FeatureStatus::Off
            } else {
                FeatureStatus::Incomplete
            },
            Vec::new(),
            format!("brain persona list -w {name}"),
        );
    };
    let roster = crate::users::UsersStore::load(&command.workspace)
        .map(|users| {
            users
                .users
                .iter()
                .map(|user| user.id.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let missing = personas.missing(&roster.iter().map(String::as_str).collect::<Vec<_>>());
    let status = if roster.is_empty() {
        FeatureStatus::Off
    } else if missing.is_empty() {
        FeatureStatus::Ready
    } else {
        FeatureStatus::Incomplete
    };
    Requirement::feature(
        RequirementScope::MemberPersonas,
        status,
        missing
            .iter()
            .map(|id| PromptMetadata::plain(format!("Persona for {id}")))
            .collect(),
        format!("brain persona set -w {name} role=<ROLE> --user <USER_ID>"),
    )
}

fn linear_status(config: &crate::config::Config) -> FeatureStatus {
    let value = config.linear_workspace.trim();
    if value.is_empty() {
        FeatureStatus::Off
    } else if config.linear_workspace_is_valid() {
        FeatureStatus::Ready
    } else {
        FeatureStatus::Incomplete
    }
}

fn populated(value: &str) -> FeatureStatus {
    if value.trim().is_empty() {
        FeatureStatus::Off
    } else {
        FeatureStatus::Ready
    }
}
