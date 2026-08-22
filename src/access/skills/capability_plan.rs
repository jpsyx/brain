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
                } else if let Some(mut skill) = machine_skills
                    .iter()
                    .find(|skill| skill.name == *name)
                    .cloned()
                {
                    match crate::skills::plugin::validate_exact_path(&skill.path) {
                        Ok(path) => {
                            skill.path = path;
                            CapabilityResolution::Available(SkillSelection::Machine(skill))
                        }
                        Err(_) => CapabilityResolution::Unavailable(
                            "machine skill path must be an absolute symlink-free directory containing a regular SKILL.md"
                                .to_owned(),
                        ),
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

pub(super) fn enforcement_entries<T>(
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
use super::{
    AccessMode, CapabilityEnforcement, CapabilityError, CapabilityPlan, CapabilityResolution,
    Config, CredentialProvenance, HashSet, MachineCapabilityEnvironment, McpCapabilityPlan,
    SkillCapabilityPlan, SkillSelection,
};
