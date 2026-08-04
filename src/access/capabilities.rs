use super::{AccessMode, boundary_prompt};
use crate::actor::ActorContext;
use crate::theme::Theme;
use crate::workspace::WorkspaceContext;

/// One immutable advisory access-policy snapshot for an agent launch.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AccessPolicy {
    mode: AccessMode,
    boundary_prompt: Option<String>,
    capability_plan: Option<super::CapabilityPlan>,
}

impl std::fmt::Debug for AccessPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessPolicy")
            .field("mode", &self.mode)
            .field(
                "boundary_prompt",
                &self.boundary_prompt.as_ref().map(|_| "<redacted>"),
            )
            .field("capability_plan", &self.capability_plan)
            .finish()
    }
}

/// Render an honest user-facing summary of the effective enforcement.
#[must_use]
pub fn render_access_status(mode: AccessMode, theme: Theme) -> String {
    let (mode, enforcement) = match mode {
        AccessMode::Unrestricted => ("unrestricted", "frontend defaults"),
        AccessMode::WorkspaceOnly => (
            "workspace-only",
            "advisory prompts and capability filtering",
        ),
    };
    format!(
        "{}  {}\n{}  {}\n{}      {}",
        theme.accent("Access mode"),
        theme.value(mode),
        theme.accent("Enforcement"),
        theme.warning(enforcement),
        theme.accent("Sandbox"),
        theme.muted("none"),
    )
}

impl AccessPolicy {
    /// Build policy only from already-resolved trusted configuration.
    #[must_use]
    pub fn new(workspace: &WorkspaceContext, actor: &ActorContext, mode: AccessMode) -> Self {
        Self {
            mode,
            boundary_prompt: boundary_prompt(workspace, actor, mode),
            capability_plan: None,
        }
    }

    /// The portable access mode captured for this launch.
    #[must_use]
    pub const fn mode(&self) -> AccessMode {
        self.mode
    }

    /// Trusted system/developer instruction, absent for unrestricted mode.
    #[must_use]
    pub fn boundary_prompt(&self) -> Option<&str> {
        self.boundary_prompt.as_deref()
    }

    /// Attach selected capability names to the trusted advisory policy.
    #[must_use]
    pub fn with_capability_plan(mut self, plan: super::CapabilityPlan) -> Self {
        if self.mode == AccessMode::WorkspaceOnly {
            let policy = format!(
                "Use only these requested MCP capabilities: {}. Use only these requested skills: {}. Capability availability and strictness are reported separately by the frontend launch.",
                display_names(&plan.mcps.names()),
                display_names(&plan.skills.names()),
            );
            match self.boundary_prompt.as_mut() {
                Some(prompt) => {
                    prompt.push_str("\n\n");
                    prompt.push_str(&policy);
                }
                None => self.boundary_prompt = Some(policy),
            }
        }
        self.capability_plan = Some(plan);
        self
    }

    /// Frontend-independent selection attached to this launch.
    #[must_use]
    pub const fn capability_plan(&self) -> Option<&super::CapabilityPlan> {
        self.capability_plan.as_ref()
    }

    pub(crate) fn matches_capability_context(
        &self,
        workspace: crate::workspace::WorkspaceId,
    ) -> bool {
        self.capability_plan.as_ref().map_or_else(
            || self.mode == AccessMode::Unrestricted,
            |plan| {
                plan.access_mode() == self.mode && plan.credentials.source_workspace() == workspace
            },
        )
    }
}

fn display_names(names: &[&str]) -> String {
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}
