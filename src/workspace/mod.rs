//! Immutable workspace identity, normalized roots, and workspace-scoped paths.

mod bootstrap;
mod bootstrap_policy;
pub mod command;
mod context;
mod id;
mod manifest;
mod name;
mod paths;
mod readiness;
pub mod registry;

pub use bootstrap::{BootstrapContext, CommandContext, bootstrap, bootstrap_with_io};
pub use bootstrap_policy::{BootstrapPolicy, Invocation, bootstrap_policy, invocation_for};
pub(crate) use context::normalize_root;
pub use context::{WorkspaceContext, WorkspaceContextError};
pub use id::{WorkspaceId, WorkspaceIdError};
pub use manifest::{MANIFEST_SCHEMA_VERSION, ManifestError, WorkspaceManifest};
pub use name::{WorkspaceName, WorkspaceNameError};
pub use paths::WorkspacePaths;
pub use readiness::{
    InteractionMode, ReadinessAction, ReadinessError, ReadinessField, readiness_action,
    readiness_action_with_users,
};
pub use registry::{
    MachineRegistry, MigrationOutcome, REGISTRY_SCHEMA_VERSION, ReceiverAction, RegistryError,
    RegistryOperation, RegistryStore, SelectedWorkspace, WorkspaceRecord, migrate_legacy,
    receiver_transition, validate_registry,
};

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{WorkspaceContext, WorkspaceId, WorkspaceName, WorkspacePaths};

    const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
    const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

    #[test]
    fn canonical_names_are_trimmed_lowercased_and_slug_validated() {
        let name = WorkspaceName::parse("  Personal_Family  ").expect("valid canonical name");

        assert_eq!(name.as_str(), "personal_family");
        for invalid in [
            "",
            "-personal",
            "_personal",
            "personal space",
            "personal!",
            "personal/",
        ] {
            assert!(
                WorkspaceName::parse(invalid).is_err(),
                "{invalid:?} must be rejected"
            );
        }
    }

    #[test]
    fn different_workspace_ids_never_share_runtime_paths() {
        let home = Path::new("/home/tester");
        let personal = WorkspacePaths::new(home, WorkspaceId::parse(PERSONAL_ID).unwrap());
        let family = WorkspacePaths::new(home, WorkspaceId::parse(FAMILY_ID).unwrap());
        let personal_base = home
            .join(".cache")
            .join("brain")
            .join("workspaces")
            .join(PERSONAL_ID);

        assert_ne!(personal.cache_dir(), family.cache_dir());
        assert_ne!(personal.state_db(), family.state_db());
        assert_ne!(personal.tui_lock(), family.tui_lock());
        assert_ne!(personal.job_socket(), family.job_socket());
        assert_ne!(
            personal.user_transaction_lock(),
            family.user_transaction_lock()
        );
        assert_ne!(personal.sync_dir(), family.sync_dir());
        assert_eq!(personal.cache_dir(), personal_base.as_path());
        assert_eq!(personal.state_db(), personal_base.join("state.db"));
        assert_eq!(personal.tui_lock(), personal_base.join("tui.lock"));
        assert_eq!(personal.job_socket(), personal_base.join("jobs.sock"));
        assert_eq!(
            personal.user_transaction_lock(),
            personal_base.join("users.transaction.lock")
        );
        assert_eq!(personal.inbox_dir(), personal_base.join("inbox"));
        assert_eq!(personal.responses_dir(), personal_base.join("responses"));
        assert_eq!(personal.logs_dir(), personal_base.join("logs"));
        assert_eq!(personal.sync_dir(), personal_base.join("sync"));
    }

    #[test]
    fn context_exposes_read_only_workspace_components() {
        let context = WorkspaceContext::new(
            Path::new("/home/tester"),
            WorkspaceId::parse(PERSONAL_ID).unwrap(),
            WorkspaceName::parse("personal").unwrap(),
            Path::new("personal"),
            "tester",
            Path::new("/workspaces"),
        )
        .expect("absolute injected base");

        let _: WorkspaceId = context.id();
        let _: &WorkspaceName = context.name();
        let _: &Path = context.root();
        let _: &str = context.local_user_id();
        let _: &WorkspacePaths = context.paths();
    }

    #[test]
    fn cache_dir_borrows_the_workspace_path() {
        let paths = WorkspacePaths::new(
            Path::new("/home/tester"),
            WorkspaceId::parse(PERSONAL_ID).unwrap(),
        );

        let _: &Path = paths.cache_dir();
    }

    #[test]
    fn contexts_keep_owned_root_snapshots_after_selected_sources_change() {
        let mut alias_selected_root = PathBuf::from("notes/../personal");
        let alias_context = WorkspaceContext::new(
            Path::new("/home/tester"),
            WorkspaceId::parse(PERSONAL_ID).unwrap(),
            WorkspaceName::parse("personal").unwrap(),
            &alias_selected_root,
            "tester",
            Path::new("/workspaces"),
        )
        .expect("absolute injected base");
        let mut default_selected_root = PathBuf::from("defaults/../family");
        let default_context = WorkspaceContext::new(
            Path::new("/home/tester"),
            WorkspaceId::parse(FAMILY_ID).unwrap(),
            WorkspaceName::parse("family").unwrap(),
            &default_selected_root,
            "tester",
            Path::new("/workspaces"),
        )
        .expect("absolute injected base");

        alias_selected_root = PathBuf::from("notes/../changed-alias");
        default_selected_root = PathBuf::from("defaults/../changed-default");

        assert_eq!(alias_context.root(), Path::new("/workspaces/personal"));
        assert_eq!(default_context.root(), Path::new("/workspaces/family"));
        assert_eq!(alias_selected_root, Path::new("notes/../changed-alias"));
        assert_eq!(
            default_selected_root,
            Path::new("defaults/../changed-default")
        );
    }
}
