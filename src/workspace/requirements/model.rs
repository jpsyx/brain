use super::super::ReadinessField;

/// A required workspace component or optional workspace feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequirementScope {
    WorkspaceRoot,
    WorkspaceManifest,
    PortableUsers,
    LocalUser,
    CloudSync,
    SyncWatcher,
    Receiver,
    Sms,
    Email,
    AccessPolicy,
    Mcp(String),
    Skill(String),
    TriageHabits,
    TriageModal,
    PdfConversion,
    Linear,
    PersonalizationRole,
    PersonalizationOrganization,
    PersonalizationTagStyles,
    BrowserViews,
    WebViews,
}

impl RequirementScope {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::WorkspaceRoot => "workspace root".to_owned(),
            Self::WorkspaceManifest => "workspace manifest".to_owned(),
            Self::PortableUsers => "portable users".to_owned(),
            Self::LocalUser => "local user".to_owned(),
            Self::CloudSync => "cloud sync".to_owned(),
            Self::SyncWatcher => "sync watcher".to_owned(),
            Self::Receiver => "receiver".to_owned(),
            Self::Sms => "SMS".to_owned(),
            Self::Email => "email".to_owned(),
            Self::AccessPolicy => "access policy (advisory; no isolation)".to_owned(),
            Self::Mcp(name) => format!("MCP {name}"),
            Self::Skill(name) => format!("skill {name}"),
            Self::TriageHabits => "managed triage habits".to_owned(),
            Self::TriageModal => "triage modal".to_owned(),
            Self::PdfConversion => "PDF conversion".to_owned(),
            Self::Linear => "Linear links".to_owned(),
            Self::PersonalizationRole => "personalization role".to_owned(),
            Self::PersonalizationOrganization => "personalization organization".to_owned(),
            Self::PersonalizationTagStyles => "personalization tag styles".to_owned(),
            Self::BrowserViews => "browser views".to_owned(),
            Self::WebViews => "web views".to_owned(),
        }
    }
}

/// Health of an optional workspace feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureStatus {
    Off,
    Ready,
    Incomplete,
}

/// Availability of a required workspace component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredStatus {
    Ready,
    Unavailable,
}

/// Required availability and optional feature health remain separate states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementStatus {
    Required(RequiredStatus),
    Feature(FeatureStatus),
}

/// One interactive input without a stored value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMetadata {
    label: String,
    secret: bool,
}

impl PromptMetadata {
    #[must_use]
    pub fn plain(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            secret: false,
        }
    }

    #[must_use]
    pub fn secret(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            secret: true,
        }
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub const fn is_secret(&self) -> bool {
        self.secret
    }
}

/// One redacted health result with human and noninteractive remediation data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    scope: RequirementScope,
    status: RequirementStatus,
    prompts: Vec<PromptMetadata>,
    remediation: String,
}

impl Requirement {
    pub(super) fn required(
        scope: RequirementScope,
        status: RequiredStatus,
        prompts: Vec<PromptMetadata>,
        remediation: String,
    ) -> Self {
        Self {
            scope,
            status: RequirementStatus::Required(status),
            prompts,
            remediation,
        }
    }

    pub(super) fn feature(
        scope: RequirementScope,
        status: FeatureStatus,
        prompts: Vec<PromptMetadata>,
        remediation: String,
    ) -> Self {
        Self {
            scope,
            status: RequirementStatus::Feature(status),
            prompts,
            remediation,
        }
    }

    #[must_use]
    pub const fn scope(&self) -> &RequirementScope {
        &self.scope
    }

    #[must_use]
    pub const fn status(&self) -> RequirementStatus {
        self.status
    }

    #[must_use]
    pub fn prompts(&self) -> &[PromptMetadata] {
        &self.prompts
    }

    #[must_use]
    pub fn remediation(&self) -> &str {
        &self.remediation
    }
}

pub(crate) fn required_fields(
    manifest_ready: bool,
    users_ready: bool,
    local_user_ready: bool,
) -> Vec<ReadinessField> {
    let mut missing = Vec::new();
    if !manifest_ready {
        missing.push(ReadinessField::Manifest);
    }
    if !users_ready {
        missing.push(ReadinessField::PortableUsers);
    }
    if !local_user_ready {
        missing.push(ReadinessField::LocalUserId);
    }
    missing
}
