use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use serde_json::Map;

use super::{
    BootstrapContext, InteractionMode, RegistryOnlyPromptOrder, bootstrap_with_io_and_hook,
    registry_only_bootstrap_with, registry_only_prompt_order,
};
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
fn bootstrap_pins_the_selected_uuid_when_the_default_changes_mid_bootstrap() {
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
    let error = crate::command::dispatch::run(cli, crate::session::AgentKind::Claude, &outcome)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unknown workspace selector family"),
        "{error:#}"
    );
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
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let cli = try_parse_from(["brain", "-b", "fam", "workspace", "list"]).unwrap();
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
