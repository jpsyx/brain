use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use serde_json::Map;

use super::{
    BootstrapContext, InteractionMode, RegistryOnlyPromptOrder, bootstrap_with_io_and_hook,
    registry_only_bootstrap_with, registry_only_prompt_order, should_migrate_global_skills,
    should_resync_skills,
};

#[test]
fn migration_bootstrap_defers_skill_writes_until_after_rollout_preflight() {
    assert!(!should_resync_skills(Invocation::WorkspaceMigrate));
    assert!(!should_resync_skills(Invocation::Tui));
    assert!(should_resync_skills(Invocation::Tasks));
    assert!(!should_migrate_global_skills(Invocation::WorkspaceMigrate));
    assert!(!should_migrate_global_skills(Invocation::Tui));
    assert!(should_migrate_global_skills(Invocation::Tasks));
}
use crate::cli::try_parse_from;
use crate::workspace::{
    Invocation, MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceId,
    WorkspaceManifest, WorkspaceName, WorkspaceRecord,
};

#[test]
fn every_prompting_registry_only_route_preflights_before_migration() {
    for invocation in [
        Invocation::WorkspaceCreate,
        Invocation::WorkspaceAttach,
        Invocation::WorkspaceRemove,
        Invocation::WorkspaceRepair,
    ] {
        assert_eq!(
            registry_only_prompt_order(invocation),
            Some(RegistryOnlyPromptOrder::BeforeMigration)
        );
    }
    assert_eq!(registry_only_prompt_order(Invocation::WorkspaceList), None);
}

#[test]
fn registry_only_bootstrap_never_runs_migration_after_preflight_cancellation() {
    let mut cli = try_parse_from(["brain", "workspace", "create"]).unwrap();
    let migration_called = Cell::new(false);
    let store = RegistryStore::from_path(std::path::PathBuf::from("/tmp/registry.json"));

    let error = registry_only_bootstrap_with(
        &mut cli,
        store,
        |_| anyhow::bail!("workspace command cancelled before the registry changed"),
        |_| {
            migration_called.set(true);
            Ok(())
        },
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cancelled before the registry changed")
    );
    assert!(!migration_called.get());
}

#[test]
fn read_only_workspace_list_skips_mutating_bootstrap_hooks() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let family_root = home.path().join("family");
    let work_root = home.path().join("work");
    std::fs::create_dir_all(&family_root).unwrap();
    std::fs::create_dir_all(&work_root).unwrap();
    let family_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let work_id = WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
    WorkspaceManifest::new(family_id)
        .write_new(&family_root)
        .unwrap();
    WorkspaceManifest::new(work_id)
        .write_new(&work_root)
        .unwrap();
    let family_name = WorkspaceName::parse("family").unwrap();
    let work_name = WorkspaceName::parse("work").unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: family_name.clone(),
        workspaces: BTreeMap::from([
            (
                family_name,
                WorkspaceRecord {
                    workspace_id: family_id,
                    root: family_root,
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env: Map::new(),
                },
            ),
            (
                work_name,
                WorkspaceRecord {
                    workspace_id: work_id,
                    root: work_root,
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env: Map::new(),
                },
            ),
        ]),
        env: serde_json::Map::new(),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let cli = try_parse_from(["brain", "workspace", "list"]).unwrap();
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io_and_hook(
        &cli,
        store.clone(),
        (home.path(), home.path()),
        InteractionMode::NonInteractive,
        &mut input,
        &mut output,
        || {
            let mut changed = RegistryStore::load_from(store.path())?;
            changed.set_default("work")?;
            changed.remove("family")?;
            store.replace(&changed)?;
            Ok(())
        },
    )
    .unwrap();

    let BootstrapContext::Ready(context) = &outcome else {
        panic!("ordinary workspace command must receive a ready context");
    };
    assert_eq!(context.workspace.id(), family_id);
    assert_eq!(context.workspace.name().as_str(), "family");
    assert_eq!(context.actor.user_id().as_str(), "pablo");
    let persisted = RegistryStore::load_from(store.path()).unwrap();
    assert_eq!(persisted.default_workspace.as_str(), "family");
    assert!(persisted.select(Some("family")).is_ok());
    crate::command::dispatch::run(cli, crate::session::AgentKind::Claude, &outcome).unwrap();
}

#[test]
fn ordinary_workspace_dispatch_never_resolves_a_removed_global_alias_again() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let family_root = home.path().join("family");
    let work_root = home.path().join("work");
    std::fs::create_dir_all(&family_root).unwrap();
    std::fs::create_dir_all(&work_root).unwrap();
    let family_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let work_id = WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
    WorkspaceManifest::new(family_id)
        .write_new(&family_root)
        .unwrap();
    WorkspaceManifest::new(work_id)
        .write_new(&work_root)
        .unwrap();
    let family_name = WorkspaceName::parse("family").unwrap();
    let work_name = WorkspaceName::parse("work").unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: work_name.clone(),
        workspaces: BTreeMap::from([
            (
                family_name,
                WorkspaceRecord {
                    workspace_id: family_id,
                    root: family_root,
                    aliases: BTreeSet::from([WorkspaceName::parse("fam").unwrap()]),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env: Map::new(),
                },
            ),
            (
                work_name,
                WorkspaceRecord {
                    workspace_id: work_id,
                    root: work_root,
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env: Map::new(),
                },
            ),
        ]),
        env: serde_json::Map::new(),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let cli = try_parse_from(["brain", "-w", "fam", "workspace", "list"]).unwrap();
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    let bootstrap = bootstrap_with_io_and_hook(
        &cli,
        store.clone(),
        (home.path(), home.path()),
        InteractionMode::NonInteractive,
        &mut input,
        &mut output,
        || {
            let mut changed = RegistryStore::load_from(store.path())?;
            changed.remove_alias("family", "fam")?;
            store.replace(&changed)?;
            Ok(())
        },
    )
    .unwrap();

    crate::command::dispatch::run(cli, crate::session::AgentKind::Claude, &bootstrap).unwrap();
}

/// One workspace with two portable members and no local user chosen yet.
fn two_member_workspace_without_a_local_user(
    home: &std::path::Path,
    config_home: &std::path::Path,
) -> RegistryStore {
    let root = home.join("family");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    WorkspaceManifest::new(workspace_id)
        .write_new(&root)
        .unwrap();
    std::fs::write(
        root.join(".config/users.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "users": [
                {"id": "pablo", "name": "Pablo", "phones": [], "emails": [], "response_email": null},
                {"id": "sun", "name": "Sun", "phones": [], "emails": [], "response_email": null}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let store = RegistryStore::from_path(config_home.join("brain/env.json"));
    store
        .replace(&MachineRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            default_workspace: WorkspaceName::parse("family").unwrap(),
            workspaces: BTreeMap::from([(
                WorkspaceName::parse("family").unwrap(),
                WorkspaceRecord {
                    workspace_id,
                    root,
                    aliases: BTreeSet::new(),
                    // The whole point: nobody has been chosen on this machine.
                    local_user_id: String::new(),
                    receiver_enabled: false,
                    env: Map::new(),
                },
            )]),
            env: Map::new(),
        })
        .unwrap();
    store
}

#[test]
fn a_command_needing_a_local_user_offers_the_roster_instead_of_asking_for_an_id() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let store = two_member_workspace_without_a_local_user(home.path(), config_home.path());
    let cli = try_parse_from(["brain", "receiver", "setup"]).unwrap();
    let mut input = Cursor::new(b"2\n".to_vec());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io_and_hook(
        &cli,
        store.clone(),
        (home.path(), home.path()),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
        || Ok(()),
    )
    .expect("a workspace with members must be repairable by picking one");

    let prompt = String::from_utf8(output).expect("UTF-8 prompt");
    // Nobody can be expected to know an ID they never typed: show who exists.
    assert!(prompt.contains("pablo"), "{prompt}");
    assert!(prompt.contains("Pablo"), "{prompt}");
    assert!(prompt.contains("sun"), "{prompt}");
    assert!(prompt.contains("1)"), "{prompt}");
    assert!(prompt.contains("2)"), "{prompt}");
    let BootstrapContext::Ready(context) = &outcome else {
        panic!("readiness repair must yield a ready context");
    };
    assert_eq!(context.actor.user_id().as_str(), "sun");
    let persisted = RegistryStore::load_from(store.path()).unwrap();
    assert_eq!(
        persisted.workspaces[&WorkspaceName::parse("family").unwrap()].local_user_id,
        "sun"
    );
}

#[test]
fn a_row_number_nobody_offered_reasks_instead_of_failing_the_command() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let store = two_member_workspace_without_a_local_user(home.path(), config_home.path());
    let cli = try_parse_from(["brain", "receiver", "setup"]).unwrap();
    let mut input = Cursor::new(b"9\n1\n".to_vec());
    let mut output = Vec::new();

    bootstrap_with_io_and_hook(
        &cli,
        store.clone(),
        (home.path(), home.path()),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
        || Ok(()),
    )
    .expect("a mistyped row must not end the command");

    let persisted = RegistryStore::load_from(store.path()).unwrap();
    assert_eq!(
        persisted.workspaces[&WorkspaceName::parse("family").unwrap()].local_user_id,
        "pablo"
    );
}
