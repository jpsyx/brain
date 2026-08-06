use super::{FeatureStatus, PromptMetadata, Requirement, RequirementScope};
use crate::access::AccessMode;
use crate::workspace::CommandContext;

pub(super) fn requirements(
    command: &CommandContext,
    name: &str,
    config: &crate::config::Config,
) -> Vec<Requirement> {
    let bundled_skills = crate::skills::embed::bundled_skills()
        .into_iter()
        .map(|skill| skill.name)
        .collect::<std::collections::BTreeSet<_>>();
    let plan = crate::access::capability_plan_for(config, command);
    let access_ready = config.access_mode == AccessMode::Unrestricted
        || crate::access::boundary_prompt(&command.workspace, &command.actor, config.access_mode)
            .is_some();
    let mut rows = vec![Requirement::feature(
        RequirementScope::AccessPolicy,
        if access_ready && plan.is_ok() {
            FeatureStatus::Ready
        } else {
            FeatureStatus::Incomplete
        },
        vec![PromptMetadata::plain("Access mode")],
        format!("brain config set -b {name} access_mode=workspace_only"),
    )];
    if config.access_mode == AccessMode::Unrestricted {
        return rows;
    }
    let Ok(plan) = plan else {
        rows.extend(config.allowed_mcps.iter().map(|capability| {
            Requirement::feature(
                RequirementScope::Mcp(capability.clone()),
                FeatureStatus::Incomplete,
                vec![PromptMetadata::secret("MCP connection material")],
                format!("brain env set -b {name} agent_capabilities=<CAPABILITY_JSON>"),
            )
        }));
        rows.extend(config.allowed_skills.iter().filter_map(|capability| {
            if bundled_skills.contains(capability) {
                return None;
            }
            Some(Requirement::feature(
                RequirementScope::Skill(capability.clone()),
                FeatureStatus::Incomplete,
                vec![PromptMetadata::plain("Skill source")],
                format!("brain env set -b {name} agent_capabilities=<CAPABILITY_JSON>"),
            ))
        }));
        return rows;
    };
    rows.extend(plan.mcps.names().into_iter().map(|capability| {
        let status = if plan.mcps.unavailable_reason(capability).is_none() {
            FeatureStatus::Ready
        } else {
            FeatureStatus::Incomplete
        };
        Requirement::feature(
            RequirementScope::Mcp(capability.to_owned()),
            status,
            vec![PromptMetadata::secret("MCP connection material")],
            format!("brain env set -b {name} agent_capabilities=<CAPABILITY_JSON>"),
        )
    }));
    rows.extend(plan.skills.names().into_iter().filter_map(|capability| {
        if bundled_skills.contains(capability) {
            return None;
        }
        let status = if plan.skills.unavailable_reason(capability).is_none() {
            FeatureStatus::Ready
        } else {
            FeatureStatus::Incomplete
        };
        Some(Requirement::feature(
            RequirementScope::Skill(capability.to_owned()),
            status,
            vec![PromptMetadata::plain("Skill source")],
            format!("brain env set -b {name} agent_capabilities=<CAPABILITY_JSON>"),
        ))
    }));
    rows
}
