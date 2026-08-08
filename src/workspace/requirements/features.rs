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
    rows.push(pdf_requirement(command, name));
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

fn pdf_requirement(command: &CommandContext, name: &str) -> Requirement {
    Requirement::feature(
        RequirementScope::PdfConversion,
        if crate::settings::configured_markdown_to_pdf_ready(command) {
            FeatureStatus::Ready
        } else {
            FeatureStatus::Incomplete
        },
        vec![PromptMetadata::plain("markdown-to-pdf executable")],
        format!("brain env set -w {name} markdown_to_pdf_path=<EXECUTABLE_PATH>"),
    )
}

fn personalization_requirements(command: &CommandContext, name: &str) -> Vec<Requirement> {
    let path = command
        .workspace
        .root()
        .join(".config/personalization.json");
    let parsed = std::fs::read(&path).ok().map(|bytes| {
        serde_json::from_slice::<crate::personalization::model::Personalization>(&bytes)
    });
    let statuses = match parsed {
        None => [FeatureStatus::Off; 3],
        Some(Ok(personalization)) => [
            populated(&personalization.role),
            populated(&personalization.works_for),
            if personalization.tag_styles.is_empty() {
                FeatureStatus::Off
            } else {
                FeatureStatus::Ready
            },
        ],
        Some(Err(_)) => [FeatureStatus::Incomplete; 3],
    };
    [
        (
            RequirementScope::PersonalizationRole,
            statuses[0],
            "Role",
            format!("brain personalize set -w {name} role=<ROLE>"),
        ),
        (
            RequirementScope::PersonalizationOrganization,
            statuses[1],
            "Organization",
            format!("brain personalize set -w {name} works_for=<ORGANIZATION>"),
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
    .collect()
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
