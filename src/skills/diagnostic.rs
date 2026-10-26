//! Public repair diagnostics contain only classified causes and validated relative artifacts.
use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Category {
    ManagedContentModified,
    OwnershipConflict,
    CorePluginOverride,
    MinimumVersion,
    Membership,
    WorkspaceIdentity,
    UnreadableSource,
    InvalidSource,
    RefreshFailed,
    ReceivePending,
    ReceiveConflict,
    ReceiptWriteFailed,
}
impl Category {
    const fn code(self) -> &'static str {
        match self {
            Self::ManagedContentModified => "managed_content_modified",
            Self::OwnershipConflict => "ownership_conflict",
            Self::CorePluginOverride => "core_plugin_override",
            Self::MinimumVersion => "minimum_version",
            Self::Membership => "membership",
            Self::WorkspaceIdentity => "workspace_identity",
            Self::UnreadableSource => "unreadable_source",
            Self::InvalidSource => "invalid_source",
            Self::RefreshFailed => "refresh_failed",
            Self::ReceivePending => "receive_pending",
            Self::ReceiveConflict => "receive_conflict",
            Self::ReceiptWriteFailed => "receipt_write_failed",
        }
    }
    const fn hint(self) -> &'static str {
        match self {
            Self::ManagedContentModified => {
                "Preserve local edits outside the generated tree, restore the named generated file, then retry brain skills sync --all-workspaces; use extensions for future customization"
            }
            Self::OwnershipConflict => {
                "Preserve the conflicting object and reconcile its ownership or file type before retry"
            }
            Self::CorePluginOverride => {
                "Rename the plugin to a unique skill name or move core customization into the matching extension, then retry"
            }
            Self::MinimumVersion => {
                "Choose a target meeting the workspace manifest minimum version, then retry"
            }
            Self::Membership => {
                "Repair portable members and the selected local actor for this workspace, then retry"
            }
            Self::WorkspaceIdentity => {
                "Reconcile the registered workspace identity with its manifest before retry"
            }
            Self::UnreadableSource => "Restore read access to the named source, then retry",
            Self::InvalidSource => {
                "Correct the named source format or unsupported file type, then retry"
            }
            Self::RefreshFailed => {
                "Inspect managed skill ownership and installation state for this workspace, then retry brain skills sync --all-workspaces"
            }
            Self::ReceivePending => {
                "Complete the interrupted receive with brain sync --pull -w <workspace> or brain sync repair -w <workspace> before running other commands or launching an agent"
            }
            Self::ReceiveConflict => {
                "The receive completed but package reconciliation failed; correct the reported cause, then retry brain skills sync --all-workspaces"
            }
            Self::ReceiptWriteFailed => {
                "Repair the local workspace state database so its receipt can be written, then retry brain skills sync --all-workspaces; the published package and recovery evidence are retained"
            }
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub category: Category,
    pub stage: &'static str,
    pub artifact: Option<String>,
    pub hint: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Box<Self>>,
}
impl Diagnostic {
    pub(crate) fn new(category: Category, stage: &'static str, artifact: Option<&str>) -> Self {
        Self {
            category,
            stage,
            artifact: artifact
                .filter(|path| safe_relative(path))
                .map(str::to_owned),
            hint: category.hint(),
            cause: None,
        }
    }
    pub(crate) fn receive_conflict(error: &anyhow::Error) -> Self {
        Self {
            cause: Some(Box::new(Self::from_error(error, "publish_package", None))),
            ..Self::new(Category::ReceiveConflict, "receive_reconcile", None)
        }
    }
    pub(crate) fn receipt_write() -> Self {
        Self::new(Category::ReceiptWriteFailed, "write_receipt", None)
    }
    pub(crate) fn from_error(
        error: &anyhow::Error,
        stage: &'static str,
        artifact: Option<&str>,
    ) -> Self {
        if let Some(diagnostic) = error.downcast_ref::<Self>() {
            return diagnostic.clone();
        }
        if let Some(error) = error.downcast_ref::<crate::workspace::ManifestError>() {
            return manifest(error, stage);
        }
        if let Some(error) = error.downcast_ref::<crate::workspace::ReadinessError>() {
            use crate::workspace::ReadinessError;
            return match error {
                ReadinessError::Manifest(error) => manifest(error, stage),
                ReadinessError::WorkspaceIdMismatch { .. } => Self::new(
                    Category::WorkspaceIdentity,
                    stage,
                    Some(".config/workspace.json"),
                ),
                _ => Self::new(Category::Membership, stage, Some(".config/users.json")),
            };
        }
        let category = if error.downcast_ref::<std::io::Error>().is_some() {
            Category::UnreadableSource
        } else if error.downcast_ref::<serde_json::Error>().is_some() {
            Category::InvalidSource
        } else {
            Category::RefreshFailed
        };
        Self::new(category, stage, artifact)
    }
    pub(crate) fn source(error: &anyhow::Error, artifact: &str) -> Self {
        if error.downcast_ref::<Self>().is_some() {
            return Self::from_error(error, "read_source", Some(artifact));
        }
        let category = if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() != std::io::ErrorKind::InvalidData)
        {
            Category::UnreadableSource
        } else {
            Category::InvalidSource
        };
        Self::new(category, "read_source", Some(artifact))
    }
}
fn manifest(error: &crate::workspace::ManifestError, stage: &'static str) -> Diagnostic {
    use crate::workspace::ManifestError;
    let category = match error {
        ManifestError::IncompatibleBrainVersion { .. } => Category::MinimumVersion,
        ManifestError::Io { .. } => Category::UnreadableSource,
        _ => Category::InvalidSource,
    };
    Diagnostic::new(category, stage, Some(".config/workspace.json"))
}
fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
impl Display for Diagnostic {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]", self.category.code(), self.stage)?;
        if let Some(artifact) = &self.artifact {
            write!(f, " {artifact}")?;
        }
        write!(f, ": {}", self.hint)?;
        if let Some(cause) = &self.cause {
            write!(f, "; cause: {cause}")?;
        }
        Ok(())
    }
}
impl std::error::Error for Diagnostic {}

#[derive(Debug)]
pub(crate) struct RefreshFailure(pub super::reconcile::RefreshReport);
impl Display for RefreshFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "managed workspace skill compatibility refresh failed")?;
        for row in &self.0.workspaces {
            if let super::reconcile::WorkspaceRefresh::Failed {
                workspace_id,
                diagnostic,
                ..
            } = &row.outcome
            {
                write!(f, "\n{} ({workspace_id}): {diagnostic}", row.name)?;
            }
        }
        if let Some(error) = &self.0.refresh_error {
            write!(f, "\n{error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for RefreshFailure {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receive_failure_hint_does_not_assume_instruction_mismatch() {
        let diagnostic = Diagnostic::new(Category::ReceiveConflict, "receive_reconcile", None);
        assert!(!diagnostic.hint.contains("AGENTS.md"));
        assert!(!diagnostic.hint.contains("matching generation"));
    }

    #[test]
    fn receive_cause_preserves_only_the_typed_safe_diagnostic() {
        for artifact in [
            ".config/extensions/workspace.md",
            "/private/secret",
            "../secret",
        ] {
            let cause = Diagnostic::new(Category::InvalidSource, "read_source", Some(artifact));
            let expected = serde_json::to_value(&cause).unwrap();
            let error = anyhow::Error::new(cause).context("private-parser-source-child-output");
            let diagnostic = Diagnostic::receive_conflict(&error);
            let value = serde_json::to_value(&diagnostic).unwrap();
            assert_eq!(value["category"], "receive_conflict");
            assert_eq!(value["cause"], expected);
            for text in [value.to_string(), diagnostic.to_string()] {
                assert!(!text.contains("private-parser"));
                assert!(!text.contains("/private/secret"));
                assert!(!text.contains("../secret"));
                assert!(!text.contains("AGENTS.md"));
            }
        }
    }

    #[test]
    fn receive_cause_classifies_unknown_errors_without_copying_their_contents() {
        for (error, category) in [
            (
                anyhow::anyhow!("private-parser-source-child-output"),
                "refresh_failed",
            ),
            (
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "private-parser-source-child-output",
                )
                .into(),
                "unreadable_source",
            ),
        ] {
            let diagnostic = Diagnostic::receive_conflict(&error);
            let value = serde_json::to_value(&diagnostic).unwrap();
            assert_eq!(value["cause"]["category"], category);
            assert_eq!(value["cause"]["stage"], "publish_package");
            assert!(value["cause"]["artifact"].is_null());
            assert!(!value.to_string().contains("private-parser"));
            assert!(!diagnostic.to_string().contains("private-parser"));
        }
    }

    #[test]
    fn diagnostics_omit_unsafe_artifacts_and_arbitrary_error_contents() {
        for artifact in [
            "/private/secret",
            "../secret",
            ".config/../secret",
            ".config/\u{1b}[31msecret",
            ".config/secret\ntext",
        ] {
            assert!(
                Diagnostic::new(Category::InvalidSource, "read_source", Some(artifact))
                    .artifact
                    .is_none()
            );
        }
        let error = anyhow::anyhow!("private-parser-source-child-output");
        assert!(
            !Diagnostic::from_error(&error, "read_source", None)
                .to_string()
                .contains("private-parser")
        );
        let error = std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "private-parser-source-child-output",
        );
        let diagnostic = Diagnostic::from_error(
            &error.into(),
            "read_source",
            Some(".config/plugins/example/SKILL.md"),
        );
        assert!(matches!(diagnostic.category, Category::UnreadableSource));
        assert!(!diagnostic.to_string().contains("private-parser"));
    }
}
