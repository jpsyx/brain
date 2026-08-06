use std::fmt::Write as _;

use super::{FeatureStatus, RequiredStatus, Requirement, RequirementStatus};

/// Render redacted requirement health grouped under one selected workspace.
#[must_use]
pub fn format_requirements(
    workspace: &super::super::WorkspaceName,
    requirements: &[Requirement],
    theme: crate::theme::Theme,
) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "{} {}",
        theme.heading("Workspace"),
        theme.accent(workspace.as_str())
    );
    for (heading, required) in [("Required", true), ("Features", false)] {
        let _ = writeln!(output, "  {}", theme.heading(heading));
        for requirement in requirements.iter().filter(|requirement| {
            matches!(requirement.status(), RequirementStatus::Required(_)) == required
        }) {
            let (symbol, status) = status_label(requirement.status(), theme);
            let _ = writeln!(
                output,
                "    {} {}: {}",
                symbol,
                requirement.scope().label(),
                status
            );
            if needs_remediation(requirement.status()) {
                let _ = writeln!(
                    output,
                    "      {} {}",
                    theme.muted("fix:"),
                    theme.accent(requirement.remediation())
                );
            }
        }
    }
    output
}

fn status_label(status: RequirementStatus, theme: crate::theme::Theme) -> (String, String) {
    match status {
        RequirementStatus::Required(RequiredStatus::Ready)
        | RequirementStatus::Feature(FeatureStatus::Ready) => {
            (theme.success("✓"), theme.success("ready"))
        }
        RequirementStatus::Required(RequiredStatus::Unavailable) => {
            (theme.error("✗"), theme.error("unavailable"))
        }
        RequirementStatus::Feature(FeatureStatus::Off) => (theme.muted("·"), theme.muted("off")),
        RequirementStatus::Feature(FeatureStatus::Incomplete) => {
            (theme.warning("!"), theme.warning("incomplete"))
        }
    }
}

const fn needs_remediation(status: RequirementStatus) -> bool {
    matches!(
        status,
        RequirementStatus::Required(RequiredStatus::Unavailable)
            | RequirementStatus::Feature(FeatureStatus::Incomplete)
    )
}
