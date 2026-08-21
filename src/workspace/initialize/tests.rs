use super::{
    RootSetup, contains_file, is_empty_workspace_inner, performs_setup_sync, root_setup,
    startup_sync_direction,
};
use crate::sync::args::Direction;

#[test]
fn an_existing_root_needs_nothing() {
    assert_eq!(root_setup(true, true), RootSetup::Ready);
    assert_eq!(root_setup(true, false), RootSetup::Ready);
}

#[test]
fn a_registered_root_whose_parent_exists_is_created() {
    // The common case: `env.json` synced from another machine names
    // `~/family`, and this machine simply does not have it yet.
    assert_eq!(root_setup(false, true), RootSetup::Create);
}

#[test]
fn a_root_under_a_missing_parent_is_reported_unavailable() {
    // An unmounted volume must not be silently replaced by an empty
    // workspace; that would look like the data was lost.
    assert_eq!(root_setup(false, false), RootSetup::Unavailable);
}

#[test]
fn a_sync_command_owns_its_own_network_run() {
    // Otherwise `brain sync` would sync twice, and seeding PARA ahead of the
    // user's own pull would manufacture empty CSVs to reconcile.
    assert!(!performs_setup_sync(crate::workspace::Invocation::Sync));
    assert!(!performs_setup_sync(
        crate::workspace::Invocation::SyncStatus
    ));
    assert!(!performs_setup_sync(crate::workspace::Invocation::Check));
}

#[test]
fn registry_management_never_writes_portable_config_as_a_side_effect() {
    // Renaming or re-defaulting a workspace is not a request to use it.
    for invocation in [
        crate::workspace::Invocation::WorkspaceRename,
        crate::workspace::Invocation::WorkspaceAlias,
        crate::workspace::Invocation::WorkspaceDefault,
        crate::workspace::Invocation::WorkspaceList,
        crate::workspace::Invocation::WorkspaceMigrate,
    ] {
        assert!(!performs_setup_sync(invocation), "{invocation:?}");
    }
}

#[test]
fn every_other_command_sets_the_workspace_up_first() {
    for invocation in [
        crate::workspace::Invocation::Tui,
        crate::workspace::Invocation::Tasks,
        crate::workspace::Invocation::Config,
        crate::workspace::Invocation::Env,
        crate::workspace::Invocation::Habits,
    ] {
        assert!(performs_setup_sync(invocation), "{invocation:?}");
    }
}

#[test]
fn without_sync_the_first_run_never_reaches_the_network() {
    assert_eq!(startup_sync_direction(false, false, true), None);
    assert_eq!(startup_sync_direction(false, false, false), None);
}

#[test]
fn the_first_sync_from_a_machine_establishes_both_directions() {
    // Local content that predates sync setup has never been uploaded, and
    // a pull-only startup would never upload it. The establishing run has
    // to move data both ways.
    assert_eq!(
        startup_sync_direction(true, false, false),
        Some(Direction::Both)
    );
    assert_eq!(
        startup_sync_direction(true, false, true),
        Some(Direction::Both)
    );
}

#[test]
fn an_empty_root_on_a_synced_machine_only_pulls() {
    // Nothing local to contribute: the remote is the source of truth.
    assert_eq!(
        startup_sync_direction(true, true, true),
        Some(Direction::Pull)
    );
}

#[test]
fn an_established_populated_workspace_adds_no_extra_startup_sync() {
    // The ordinary startup pull and the change-triggered push already own
    // this case; syncing again here would sync twice on every command.
    assert_eq!(startup_sync_direction(true, true, false), None);
}

#[test]
fn setup_only_directories_are_empty() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".config")).unwrap();
    std::fs::create_dir(root.path().join(".claude")).unwrap();
    std::fs::create_dir_all(root.path().join(".brain/hooks")).unwrap();
    std::fs::write(
        root.path().join(".brain/hooks/agent_session_start_hook.py"),
        "# managed lifecycle hook\n",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("tasks")).unwrap();
    assert!(is_empty_workspace_inner(root.path()).unwrap());
}

#[test]
fn a_user_file_prevents_initialization() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("areas")).unwrap();
    std::fs::write(root.path().join("areas/family.md"), "family").unwrap();
    assert!(!is_empty_workspace_inner(root.path()).unwrap());
}

#[test]
fn nested_empty_para_directories_have_no_files() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("projects/empty")).unwrap();
    assert!(!contains_file(root.path().join("projects")).unwrap());
}
