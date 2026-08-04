use super::{AccessMode, boundary_prompt};
use crate::actor::ActorContext;
use crate::theme::Theme;
use crate::workspace::WorkspaceContext;

/// One immutable advisory access-policy snapshot for an agent launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessPolicy {
    mode: AccessMode,
    boundary_prompt: Option<String>,
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
}
