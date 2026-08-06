//! Logical workspace capability resolution, independent of frontend enforcement.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::config::Config;
use crate::workspace::WorkspaceId;

use super::AccessMode;
use super::enforcement::{
    CapabilityEnforcement, CapabilityEnforcementReport, CapabilityEnforcementSet,
    EnforcementEvidence,
};
use super::mcp::{MachineCapabilityEnvironment, MachineMcp, MachineSkill};

/// Invalid portable or machine-local capability configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityError {
    /// The selected record's machine-local object did not match its schema.
    InvalidMachineEnvironment(String),
    /// A logical name appeared more than once in one configuration field.
    DuplicateLogicalName {
        /// Field containing the ambiguity.
        field: &'static str,
        /// Repeated logical name.
        name: String,
    },
    /// A logical name did not use the portable capability-name grammar.
    InvalidLogicalName {
        /// Field containing the invalid value.
        field: &'static str,
    },
    /// A cache-local capability artifact could not be written safely.
    RuntimeArtifact(String),
}

impl Display for CapabilityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMachineEnvironment(message) => {
                write!(
                    formatter,
                    "invalid machine capability environment: {message}"
                )
            }
            Self::DuplicateLogicalName { field, name } => {
                write!(formatter, "duplicate {field} name `{name}`")
            }
            Self::InvalidLogicalName { field } => {
                write!(
                    formatter,
                    "{field} names must use ASCII letters or digits with internal `.`, `_`, or `-`"
                )
            }
            Self::RuntimeArtifact(message) => {
                write!(formatter, "write workspace capability artifact: {message}")
            }
        }
    }
}

impl Error for CapabilityError {}

/// Machine-record provenance retained without exposing credential values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialProvenance {
    source_workspace: WorkspaceId,
}

impl CredentialProvenance {
    /// Workspace record from which connection material was resolved.
    #[must_use]
    pub const fn source_workspace(self) -> WorkspaceId {
        self.source_workspace
    }
}

/// Requested MCP capabilities and their selected machine material.
#[derive(Clone, PartialEq, Eq)]
pub struct McpCapabilityPlan {
    entries: Vec<(String, CapabilityResolution<MachineMcp>)>,
    global_configuration: bool,
}

impl std::fmt::Debug for McpCapabilityPlan {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpCapabilityPlan")
            .field("names", &self.names())
            .field("available_names", &self.available_names())
            .field("global_configuration", &self.global_configuration)
            .finish_non_exhaustive()
    }
}

impl McpCapabilityPlan {
    /// Requested logical names, never the unselected machine inventory.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// Whether the frontend should use its ordinary global configuration.
    #[must_use]
    pub const fn uses_global_configuration(&self) -> bool {
        self.global_configuration
    }

    /// Requested names with complete machine-local connection material.
    #[must_use]
    pub fn available_names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter_map(|(name, resolution)| {
                matches!(resolution, CapabilityResolution::Available(_)).then_some(name.as_str())
            })
            .collect()
    }

    /// Honest reason a requested capability cannot be launched.
    #[must_use]
    pub fn unavailable_reason(&self, requested: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(name, _)| name == requested)
            .and_then(|(_, resolution)| match resolution {
                CapabilityResolution::Available(_) => None,
                CapabilityResolution::Unavailable(reason) => Some(reason.as_str()),
            })
    }

    pub(crate) fn available_connections(&self) -> impl Iterator<Item = &MachineMcp> {
        self.entries
            .iter()
            .filter_map(|(_, resolution)| match resolution {
                CapabilityResolution::Available(connection) => Some(connection),
                CapabilityResolution::Unavailable(_) => None,
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CapabilityResolution<T> {
    Available(T),
    Unavailable(String),
}

/// Requested skill capabilities and their selected sources.
#[derive(Clone, PartialEq, Eq)]
pub struct SkillCapabilityPlan {
    entries: Vec<(String, CapabilityResolution<SkillSelection>)>,
    global_configuration: bool,
}

impl std::fmt::Debug for SkillCapabilityPlan {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SkillCapabilityPlan")
            .field("names", &self.names())
            .field("available_names", &self.available_names())
            .field("global_configuration", &self.global_configuration)
            .finish_non_exhaustive()
    }
}

impl SkillCapabilityPlan {
    /// Requested logical names, preserving portable allowlist order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// Whether the frontend should use its ordinary global configuration.
    #[must_use]
    pub const fn uses_global_configuration(&self) -> bool {
        self.global_configuration
    }

    /// Requested names with an available bundled or machine-local source.
    #[must_use]
    pub fn available_names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter_map(|(name, resolution)| {
                matches!(resolution, CapabilityResolution::Available(_)).then_some(name.as_str())
            })
            .collect()
    }

    /// Honest reason a requested skill source cannot be rendered.
    #[must_use]
    pub fn unavailable_reason(&self, requested: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(name, _)| name == requested)
            .and_then(|(_, resolution)| match resolution {
                CapabilityResolution::Available(_) => None,
                CapabilityResolution::Unavailable(reason) => Some(reason.as_str()),
            })
    }

    pub(crate) fn available_sources(&self) -> Vec<ResolvedSkillSource> {
        self.entries
            .iter()
            .filter_map(|(name, resolution)| match resolution {
                CapabilityResolution::Available(SkillSelection::Bundled) => {
                    Some(ResolvedSkillSource::Bundled { name: name.clone() })
                }
                CapabilityResolution::Available(SkillSelection::Machine(skill)) => {
                    Some(ResolvedSkillSource::Machine {
                        name: name.clone(),
                        path: skill.path.clone(),
                    })
                }
                CapabilityResolution::Unavailable(_) => None,
            })
            .collect()
    }
}

pub(crate) enum ResolvedSkillSource {
    Bundled {
        name: String,
    },
    Machine {
        name: String,
        path: std::path::PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SkillSelection {
    Bundled,
    Machine(MachineSkill),
}

/// Frontend-independent capability selection for one launch.
#[derive(Clone, PartialEq, Eq)]
pub struct CapabilityPlan {
    access_mode: AccessMode,
    pub mcps: McpCapabilityPlan,
    pub skills: SkillCapabilityPlan,
    pub credentials: CredentialProvenance,
}

impl std::fmt::Debug for CapabilityPlan {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapabilityPlan")
            .field("access_mode", &self.access_mode)
            .field("mcps", &self.mcps)
            .field("skills", &self.skills)
            .field("credentials", &self.credentials)
            .finish()
    }
}

impl CapabilityPlan {
    /// Portable access mode this plan was resolved for.
    #[must_use]
    pub const fn access_mode(&self) -> AccessMode {
        self.access_mode
    }

    /// Map selected availability through facts proven by actual launch flags.
    #[must_use]
    pub fn enforcement_report(&self, evidence: EnforcementEvidence) -> CapabilityEnforcementReport {
        CapabilityEnforcementReport {
            mcps: CapabilityEnforcementSet {
                entries: enforcement_entries(&self.mcps.entries, evidence.strict_mcps),
            },
            skills: CapabilityEnforcementSet {
                entries: enforcement_entries(&self.skills.entries, evidence.strict_skills),
            },
        }
    }
}

/// Resolve logical portable names against the selected machine record.
///
/// # Errors
///
/// Returns a capability configuration error when logical or machine names are
/// ambiguous.
mod capability_plan;
use capability_plan::enforcement_entries;
pub use capability_plan::{capability_plan, capability_plan_for};
