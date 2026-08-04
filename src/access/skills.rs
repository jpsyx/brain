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
pub fn capability_plan(
    config: &Config,
    machine: &MachineCapabilityEnvironment,
) -> Result<CapabilityPlan, CapabilityError> {
    let allowed_mcps = normalize_names("allowed_mcps", &config.allowed_mcps)?;
    let allowed_skills = normalize_names("allowed_skills", &config.allowed_skills)?;
    let machine_mcp_names = normalize_names(
        "machine MCP",
        &machine
            .mcps
            .iter()
            .map(|mcp| mcp.name.clone())
            .collect::<Vec<_>>(),
    )?;
    let machine_skill_names = normalize_names(
        "machine skill",
        &machine
            .skills
            .iter()
            .map(|skill| skill.name.clone())
            .collect::<Vec<_>>(),
    )?;
    let machine_mcps = machine
        .mcps
        .iter()
        .cloned()
        .zip(machine_mcp_names)
        .map(|(mut mcp, name)| {
            mcp.name = name;
            mcp
        })
        .collect::<Vec<_>>();
    let machine_skills = machine
        .skills
        .iter()
        .cloned()
        .zip(machine_skill_names)
        .map(|(mut skill, name)| {
            skill.name = name;
            skill
        })
        .collect::<Vec<_>>();
    let global_configuration = config.access_mode == AccessMode::Unrestricted;
    let mcps = if global_configuration {
        Vec::new()
    } else {
        allowed_mcps
            .iter()
            .map(|name| {
                let selected = machine_mcps.iter().find(|mcp| mcp.name == *name).cloned();
                let resolution = selected.map_or_else(
                    || {
                        CapabilityResolution::Unavailable(
                            "machine connection is not configured".to_owned(),
                        )
                    },
                    |connection| {
                        connection.unavailable_reason().map_or_else(
                            || CapabilityResolution::Available(connection),
                            CapabilityResolution::Unavailable,
                        )
                    },
                );
                (name.clone(), resolution)
            })
            .collect()
    };
    let skills = if global_configuration {
        Vec::new()
    } else {
        let bundled = crate::skills::embed::bundled_skills()
            .into_iter()
            .map(|skill| skill.name)
            .collect::<HashSet<_>>();
        allowed_skills
            .iter()
            .map(|name| {
                let resolution = if bundled.contains(name) {
                    CapabilityResolution::Available(SkillSelection::Bundled)
                } else if let Some(skill) = machine_skills
                    .iter()
                    .find(|skill| skill.name == *name)
                    .cloned()
                {
                    if crate::skills::plugin::validate_exact(&skill.path).is_ok() {
                        CapabilityResolution::Available(SkillSelection::Machine(skill))
                    } else {
                        CapabilityResolution::Unavailable(
                            "machine skill path must be an absolute symlink-free directory containing a regular SKILL.md"
                                .to_owned(),
                        )
                    }
                } else {
                    CapabilityResolution::Unavailable(
                        "machine skill source is not configured".to_owned(),
                    )
                };
                (name.clone(), resolution)
            })
            .collect()
    };
    Ok(CapabilityPlan {
        access_mode: config.access_mode,
        mcps: McpCapabilityPlan {
            entries: mcps,
            global_configuration,
        },
        skills: SkillCapabilityPlan {
            entries: skills,
            global_configuration,
        },
        credentials: CredentialProvenance {
            source_workspace: machine.source_workspace(),
        },
    })
}

/// Resolve one selected command context without consulting another record.
///
/// # Errors
///
/// Returns a capability configuration error when the selected record's
/// machine-local material or the portable logical lists are invalid.
pub fn capability_plan_for(
    config: &Config,
    command: &crate::workspace::CommandContext,
) -> Result<CapabilityPlan, CapabilityError> {
    let env = crate::env::load_map(command);
    let machine = MachineCapabilityEnvironment::from_selected_map(command.workspace.id(), &env)?;
    capability_plan(config, &machine)
}

fn enforcement_entries<T>(
    entries: &[(String, CapabilityResolution<T>)],
    strict: bool,
) -> Vec<(String, CapabilityEnforcement)> {
    entries
        .iter()
        .map(|(name, resolution)| {
            let enforcement = match resolution {
                CapabilityResolution::Available(_) if strict => {
                    CapabilityEnforcement::StrictlySelected
                }
                CapabilityResolution::Available(_) => CapabilityEnforcement::AdvisoryOnly,
                CapabilityResolution::Unavailable(_) => CapabilityEnforcement::Unavailable,
            };
            (name.clone(), enforcement)
        })
        .collect()
}

fn normalize_names(field: &'static str, names: &[String]) -> Result<Vec<String>, CapabilityError> {
    let mut seen = HashSet::with_capacity(names.len());
    let mut normalized = Vec::with_capacity(names.len());
    for name in names {
        let mut bytes = name.bytes();
        let valid = name.len() <= 128
            && bytes
                .next()
                .is_some_and(|first| first.is_ascii_alphanumeric())
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        if !valid {
            return Err(CapabilityError::InvalidLogicalName { field });
        }
        let name = name.to_ascii_lowercase();
        if !seen.insert(name.clone()) {
            return Err(CapabilityError::DuplicateLogicalName { field, name });
        }
        normalized.push(name);
    }
    Ok(normalized)
}
