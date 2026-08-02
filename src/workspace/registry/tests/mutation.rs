use super::*;

#[test]
fn rename_rekeys_canonical_name_and_preserves_the_complete_record() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");
    let mut registry = registry_with_brain_and_family();
    let original = registry.workspaces.get(&name("family")).unwrap().clone();
    RegistryStore::save_atomic_to(&path, &registry).expect("initial save");
    let store = RegistryStore::from_path(path.clone());

    store
        .update(&mut registry, |candidate| {
            candidate.rename("family", name("household"))
        })
        .expect("valid rename");

    assert!(!registry.workspaces.contains_key(&name("family")));
    assert_eq!(registry.workspaces.get(&name("household")), Some(&original));
    assert_eq!(RegistryStore::load_from(&path).unwrap(), registry);
}

#[test]
fn changing_default_preserves_all_workspace_records() {
    let mut registry = registry_with_brain_and_family();
    let original_records = registry.workspaces.clone();

    registry.set_default("family").expect("canonical workspace");

    assert_eq!(registry.default_workspace, name("family"));
    assert_eq!(registry.workspaces, original_records);
}

#[test]
fn removal_never_touches_workspace_root_contents() {
    let directory = tempfile::tempdir().expect("tempdir");
    let root = directory.path().join("family");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("keep.txt"), b"untouched").unwrap();
    let mut registry = registry_with_brain_and_family();
    registry.workspaces.get_mut(&name("family")).unwrap().root = root.clone();

    let removed = registry.remove("family").expect("non-default removal");

    assert_eq!(removed.workspace_id, id(FAMILY_ID));
    assert_eq!(fs::read(root.join("keep.txt")).unwrap(), b"untouched");
}

#[test]
fn record_creation_attachment_and_alias_mutations_validate_the_registry() {
    let mut registry = registry_with_brain_and_family();
    let created_id = registry
        .create_record(name("work"), PathBuf::from("/workspaces/work"), "pablo")
        .expect("create record");
    let attached = record(
        "f7dc5520-e4d1-4a60-bb5a-2f121436747d",
        "/workspaces/shared",
        "shared",
    );

    registry
        .attach_record(name("shared"), attached.clone())
        .expect("attach record");
    registry.add_alias("shared", name("team")).unwrap();
    registry.remove_alias("shared", "team").unwrap();

    assert_eq!(
        registry.workspaces.get(&name("work")).unwrap().workspace_id,
        created_id
    );
    assert_eq!(registry.workspaces.get(&name("shared")), Some(&attached));
    assert!(validate_registry(&registry).is_ok());
}
