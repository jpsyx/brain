//! Registry of functional agent frontends and their integration contracts.

mod contract;

use contract::{
    CLAUDE_HEALTH, CLAUDE_LIFECYCLE, CODEX_HEALTH, CODEX_LIFECYCLE, OPENCODE_HEALTH,
    OPENCODE_LIFECYCLE,
};
pub(crate) use contract::{
    HealthCheckDescriptor, HealthCheckExpectation, HookCommandStyle, LifecycleInstallation,
    LifecyclePayload,
};

use crate::{
    access::EnforcementEvidence,
    agent::{
        AgentFrontend, AgentKind, ClaudeFrontend, CodexFrontend, OpenCodeFrontend, SessionPlan,
    },
    workspace::{CommandContext, WorkspaceContext},
};

type FrontendConstructor = fn(&WorkspaceContext, String) -> Box<dyn AgentFrontend>;
type CommandBuilder = fn(&str, &SessionPlan, Option<&str>) -> String;
type CapabilityEvidence = fn(&str) -> EnforcementEvidence;
type CompatibilityProbe = fn(&str) -> Result<Option<String>, crate::agent::AgentError>;

/// Complete construction and integration contract for one functional frontend.
pub(crate) struct FrontendRegistration {
    kind: AgentKind,
    label: &'static str,
    command_key: &'static str,
    default_command: &'static str,
    constructor: FrontendConstructor,
    command_builder: CommandBuilder,
    lifecycle: &'static [LifecycleInstallation],
    health_checks: &'static [HealthCheckDescriptor],
    capability_evidence: CapabilityEvidence,
    compatibility_probe: Option<CompatibilityProbe>,
}

impl FrontendRegistration {
    #[must_use]
    pub(crate) const fn kind(&self) -> AgentKind {
        self.kind
    }

    #[must_use]
    pub(crate) const fn label(&self) -> &'static str {
        self.label
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn command_key(&self) -> &'static str {
        self.command_key
    }

    #[must_use]
    pub(crate) const fn default_command(&self) -> &'static str {
        self.default_command
    }

    #[must_use]
    pub(crate) fn configured_command(&self, command: &CommandContext) -> String {
        crate::env::resolve_one(command, self.command_key)
            .unwrap_or_else(|| self.default_command.to_owned())
    }

    #[must_use]
    pub(crate) fn frontend(&self, command: &CommandContext) -> Box<dyn AgentFrontend> {
        (self.constructor)(&command.workspace, self.configured_command(command))
    }

    #[must_use]
    pub(crate) const fn lifecycle(&self) -> &'static [LifecycleInstallation] {
        self.lifecycle
    }

    #[must_use]
    pub(crate) const fn health_checks(&self) -> &'static [HealthCheckDescriptor] {
        self.health_checks
    }

    #[must_use]
    pub(crate) const fn capability_evidence(&self) -> CapabilityEvidence {
        self.capability_evidence
    }

    pub(crate) fn compatibility(
        &self,
        command: &str,
    ) -> Option<Result<Option<String>, crate::agent::AgentError>> {
        self.compatibility_probe.map(|probe| probe(command))
    }

    #[must_use]
    pub(crate) const fn requires_compatibility_probe(&self) -> bool {
        self.compatibility_probe.is_some()
    }

    #[must_use]
    pub(crate) const fn frontend_constructor(&self) -> FrontendConstructor {
        self.constructor
    }

    #[must_use]
    pub(crate) fn build_command(
        &self,
        configured_command: &str,
        plan: &SessionPlan,
        prompt: Option<&str>,
    ) -> String {
        (self.command_builder)(configured_command, plan, prompt)
    }
}

fn claude_frontend(workspace: &WorkspaceContext, configured: String) -> Box<dyn AgentFrontend> {
    let workspace_root = workspace.root().to_path_buf();
    match std::env::var_os("HOME") {
        Some(home) => Box::new(ClaudeFrontend::new(
            configured,
            workspace_root,
            std::path::PathBuf::from(home)
                .join(".claude")
                .join("projects"),
        )),
        None => Box::new(ClaudeFrontend::without_projects_dir(
            configured,
            workspace_root,
        )),
    }
}

fn codex_frontend(_workspace: &WorkspaceContext, configured: String) -> Box<dyn AgentFrontend> {
    Box::new(CodexFrontend::new(configured))
}

fn opencode_frontend(workspace: &WorkspaceContext, configured: String) -> Box<dyn AgentFrontend> {
    Box::new(OpenCodeFrontend::for_workspace(
        configured,
        workspace.root(),
    ))
}

const fn advisory_evidence(_command: &str) -> EnforcementEvidence {
    EnforcementEvidence::advisory_only()
}

static REGISTRATIONS: [FrontendRegistration; 3] = [
    FrontendRegistration {
        kind: AgentKind::Claude,
        label: "Claude",
        command_key: "claude_cmd",
        default_command: super::DEFAULT_CLAUDE_COMMAND,
        constructor: claude_frontend,
        command_builder: ClaudeFrontend::command_for,
        lifecycle: &CLAUDE_LIFECYCLE,
        health_checks: &CLAUDE_HEALTH,
        capability_evidence: ClaudeFrontend::mcp_enforcement_evidence,
        compatibility_probe: None,
    },
    FrontendRegistration {
        kind: AgentKind::Codex,
        label: "Codex",
        command_key: "codex_cmd",
        default_command: super::DEFAULT_CODEX_COMMAND,
        constructor: codex_frontend,
        command_builder: CodexFrontend::command_for,
        lifecycle: &CODEX_LIFECYCLE,
        health_checks: &CODEX_HEALTH,
        capability_evidence: advisory_evidence,
        compatibility_probe: None,
    },
    FrontendRegistration {
        kind: AgentKind::OpenCode,
        label: "OpenCode",
        command_key: "opencode_cmd",
        default_command: super::DEFAULT_OPENCODE_COMMAND,
        constructor: opencode_frontend,
        command_builder: OpenCodeFrontend::command_for,
        lifecycle: &OPENCODE_LIFECYCLE,
        health_checks: &OPENCODE_HEALTH,
        capability_evidence: advisory_evidence,
        compatibility_probe: Some(super::opencode_compatibility_version),
    },
];

/// Every frontend registration in stable display order.
#[must_use]
pub(crate) const fn registrations() -> &'static [FrontendRegistration; 3] {
    &REGISTRATIONS
}

/// Registration for one selected frontend.
#[must_use]
pub(crate) fn registration(kind: AgentKind) -> &'static FrontendRegistration {
    REGISTRATIONS
        .iter()
        .find(|registration| registration.kind == kind)
        .expect("AgentKind::ALL and the frontend registry stay exhaustive")
}

pub(crate) fn primary_session_health_check() -> Option<HealthCheckDescriptor> {
    REGISTRATIONS
        .iter()
        .flat_map(FrontendRegistration::health_checks)
        .copied()
        .find(|descriptor| {
            matches!(
                descriptor.expectation(),
                HealthCheckExpectation::Hook {
                    event: "SessionStart",
                    ..
                }
            )
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn registry_covers_every_frontend_with_complete_integration_metadata() {
        let registrations = registrations();
        assert_eq!(registrations.len(), AgentKind::ALL.len());
        assert_eq!(
            registrations
                .iter()
                .map(FrontendRegistration::kind)
                .collect::<Vec<_>>(),
            AgentKind::ALL
        );

        for registration in registrations {
            assert!(!registration.label().is_empty());
            assert!(!registration.command_key().is_empty());
            assert!(!registration.default_command().is_empty());
            assert!(!registration.lifecycle().is_empty());
            assert!(!registration.health_checks().is_empty());
            let _ = registration.capability_evidence()(registration.default_command());
            let _ = registration.frontend_constructor();
        }
    }

    #[test]
    fn lifecycle_installation_ids_are_globally_unique() {
        let installations = registrations()
            .iter()
            .flat_map(FrontendRegistration::lifecycle)
            .collect::<Vec<_>>();
        let ids = installations
            .iter()
            .map(|installation| installation.id())
            .collect::<BTreeSet<_>>();

        assert_eq!(ids.len(), installations.len());
    }
}
