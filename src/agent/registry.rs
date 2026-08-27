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
        compatibility_probe: Some(super::claude_compatibility_version),
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
    use std::{collections::BTreeSet, os::unix::fs::PermissionsExt as _};

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

    #[test]
    fn registry_probes_claude_and_opencode_compatibility_but_leaves_codex_unchanged() {
        assert!(
            registration(AgentKind::Claude).requires_compatibility_probe(),
            "Claude receiver hooks require the prompt_id compatibility floor"
        );
        assert!(
            !registration(AgentKind::Codex).requires_compatibility_probe(),
            "Codex compatibility remains artifact-declared"
        );
        assert!(
            registration(AgentKind::OpenCode).requires_compatibility_probe(),
            "OpenCode retains its existing capability probe"
        );
    }

    fn fake_claude_command(version_output: &str) -> (tempfile::TempDir, String) {
        let temporary = tempfile::tempdir().expect("temporary Claude command");
        let script = temporary.path().join("fake claude");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ \"$1\" = --profile ] || exit 64\n[ \"$2\" = brain ] || exit 65\n[ \"$3\" = --version ] || exit 66\nprintf '%s\\n' '{}'\n",
                version_output.replace('\'', "'\\''")
            ),
        )
        .expect("write fake Claude command");
        let mut permissions = std::fs::metadata(&script)
            .expect("fake Claude metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).expect("make fake Claude executable");
        let command = format!(
            "{} --profile brain",
            crate::agent::frontend::shell_quote(&script.display().to_string())
        );
        (temporary, command)
    }

    fn claude_compatibility(command: &str) -> Result<Option<String>, crate::agent::AgentError> {
        registration(AgentKind::Claude)
            .compatibility(command)
            .expect("Claude registry compatibility probe")
    }

    #[test]
    fn claude_compatibility_rejects_the_version_before_prompt_id_support() {
        let (_temporary, command) = fake_claude_command("2.1.195 (Claude Code)");

        let error = claude_compatibility(&command).expect_err("Claude below version floor");
        let message = error.to_string();

        assert_eq!(
            message,
            "frontend error: Claude is incompatible: version 2.1.195 does not provide the required `prompt_id` hook field. Update Claude Code to 2.1.196 or later, or set `brain env set claude_cmd <command>` to a compatible command."
        );
        assert!(!message.contains("fake claude"));
    }

    #[test]
    fn claude_compatibility_accepts_the_exact_prompt_id_minimum() {
        let (_temporary, command) = fake_claude_command("2.1.196 (Claude Code)");

        assert_eq!(
            claude_compatibility(&command),
            Ok(Some("2.1.196".to_owned()))
        );
    }

    #[test]
    fn claude_compatibility_accepts_a_newer_version() {
        let (_temporary, command) = fake_claude_command("3.4.5 (Claude Code)");

        assert_eq!(claude_compatibility(&command), Ok(Some("3.4.5".to_owned())));
    }

    #[test]
    fn claude_compatibility_rejects_malformed_version_output() {
        let (_temporary, command) = fake_claude_command("Claude Code current");

        assert_eq!(
            claude_compatibility(&command)
                .expect_err("malformed Claude version")
                .to_string(),
            "frontend error: Claude is incompatible: the configured command returned an unrecognized version. Update Claude Code to 2.1.196 or later, or set `brain env set claude_cmd <command>` to a compatible command."
        );
    }

    #[test]
    fn claude_compatibility_rejects_numeric_output_without_claude_identity() {
        let (_temporary, command) = fake_claude_command("Python 3.9.6");

        assert_eq!(
            claude_compatibility(&command)
                .expect_err("numeric output from a non-Claude command")
                .to_string(),
            "frontend error: Claude is incompatible: the configured command returned an unrecognized version. Update Claude Code to 2.1.196 or later, or set `brain env set claude_cmd <command>` to a compatible command."
        );
    }

    #[test]
    fn claude_compatibility_rejects_noisy_or_ambiguous_wrapper_output() {
        let (_temporary, command) = fake_claude_command("wrapper 9.9.9\n2.1.195 (Claude Code)");

        assert_eq!(
            claude_compatibility(&command)
                .expect_err("wrapper output with multiple numeric versions")
                .to_string(),
            "frontend error: Claude is incompatible: the configured command returned an unrecognized version. Update Claude Code to 2.1.196 or later, or set `brain env set claude_cmd <command>` to a compatible command."
        );
    }

    #[test]
    fn claude_compatibility_rejects_an_unavailable_command() {
        assert_eq!(
            claude_compatibility("brain-missing-claude-command-9bde08bd")
                .expect_err("unavailable Claude command")
                .to_string(),
            "frontend error: Claude is unavailable: the configured command could not run. Install Claude Code 2.1.196 or later, or set `brain env set claude_cmd <command>` to a compatible command."
        );
    }
}
