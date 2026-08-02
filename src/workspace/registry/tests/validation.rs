use super::*;

#[test]
fn alias_selects_exactly_one_canonical_workspace() {
    let registry = registry_with_brain_and_family();

    let selected = registry.select(Some("FAM")).expect("alias selection");

    assert_eq!(selected.canonical_name().as_str(), "family");
    assert!(std::ptr::eq(
        selected.record(),
        registry.workspaces.get(&name("family")).unwrap()
    ));
}

#[test]
fn omitted_selector_selects_default_without_merging_environment() {
    let registry = registry_with_brain_and_family();

    let selected = registry.select(None).expect("default selection");

    assert_eq!(selected.canonical_name().as_str(), "brain");
    assert_eq!(
        selected.record().env,
        Map::from_iter([("sentinel".to_owned(), json!("personal"))])
    );
}

#[test]
fn unknown_selector_error_carries_the_requested_selector() {
    let error = registry_with_brain_and_family()
        .select(Some("missing"))
        .unwrap_err();

    assert_eq!(
        error,
        RegistryError::UnknownSelector {
            selector: "missing".to_owned()
        }
    );
}

#[test]
fn aliases_colliding_under_ascii_case_folding_are_rejected() {
    let mut registry = registry_with_brain_and_family();
    registry
        .workspaces
        .get_mut(&name("brain"))
        .unwrap()
        .aliases
        .insert(name("FAM"));

    assert!(matches!(
        validate_registry(&registry),
        Err(RegistryError::DuplicateSelector { selector, .. }) if selector == "fam"
    ));
}

#[test]
fn canonical_name_and_alias_collision_under_ascii_case_folding_is_rejected() {
    let mut registry = registry_with_brain_and_family();
    registry
        .workspaces
        .get_mut(&name("brain"))
        .unwrap()
        .aliases
        .insert(name("FAMILY"));

    assert!(matches!(
        validate_registry(&registry),
        Err(RegistryError::DuplicateSelector { selector, .. }) if selector == "family"
    ));
}

#[test]
fn duplicate_workspace_ids_are_rejected() {
    let mut registry = registry_with_brain_and_family();
    registry
        .workspaces
        .get_mut(&name("family"))
        .unwrap()
        .workspace_id = id(PERSONAL_ID);

    assert_eq!(
        validate_registry(&registry),
        Err(RegistryError::DuplicateWorkspaceId {
            workspace_id: id(PERSONAL_ID)
        })
    );
}

#[test]
fn missing_default_is_rejected() {
    let mut registry = registry_with_brain_and_family();
    registry.default_workspace = name("missing");

    assert_eq!(
        validate_registry(&registry),
        Err(RegistryError::MissingDefault {
            default_workspace: name("missing")
        })
    );
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let mut registry = registry_with_brain_and_family();
    registry.schema_version = 1;

    assert_eq!(
        validate_registry(&registry),
        Err(RegistryError::UnsupportedSchemaVersion { found: 1 })
    );
}

#[test]
fn empty_registry_is_rejected_with_the_exact_variant() {
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: name("brain"),
        workspaces: BTreeMap::new(),
    };

    assert_eq!(
        validate_registry(&registry),
        Err(RegistryError::EmptyRegistry)
    );
}

#[test]
fn relative_root_is_rejected_with_the_exact_variant() {
    let mut registry = registry_with_brain_and_family();
    registry.workspaces.get_mut(&name("family")).unwrap().root = PathBuf::from("relative/family");

    assert_eq!(
        validate_registry(&registry),
        Err(RegistryError::RelativeRoot {
            canonical_name: name("family"),
            root: PathBuf::from("relative/family"),
        })
    );
}

#[test]
fn exact_duplicate_roots_are_rejected_after_lexical_normalization() {
    let mut registry = registry_with_brain_and_family();
    registry.workspaces.get_mut(&name("family")).unwrap().root =
        PathBuf::from("/workspaces/notes/../brain");

    assert!(matches!(
        validate_registry(&registry),
        Err(RegistryError::OverlappingRoots { first, second })
            if first == Path::new("/workspaces/brain") && second == Path::new("/workspaces/brain")
    ));
}

#[test]
fn ancestor_and_descendant_roots_are_rejected() {
    let mut registry = registry_with_brain_and_family();
    registry.workspaces.get_mut(&name("family")).unwrap().root =
        PathBuf::from("/workspaces/brain/family");

    assert!(matches!(
        validate_registry(&registry),
        Err(RegistryError::OverlappingRoots { first, second })
            if first == Path::new("/workspaces/brain")
                && second == Path::new("/workspaces/brain/family")
    ));
}

#[test]
fn filesystem_root_and_descendant_are_rejected() {
    let mut registry = registry_with_brain_and_family();
    registry.workspaces.get_mut(&name("brain")).unwrap().root = PathBuf::from("/");
    registry.workspaces.get_mut(&name("family")).unwrap().root = PathBuf::from("/family");

    assert!(matches!(
        validate_registry(&registry),
        Err(RegistryError::OverlappingRoots { first, second })
            if first == Path::new("/") && second == Path::new("/family")
    ));
}

#[test]
fn sibling_path_prefixes_are_accepted() {
    let mut registry = registry_with_brain_and_family();
    registry.workspaces.get_mut(&name("brain")).unwrap().root = PathBuf::from("/workspaces/a");
    registry.workspaces.get_mut(&name("family")).unwrap().root = PathBuf::from("/workspaces/ab");

    assert!(validate_registry(&registry).is_ok());
}

#[test]
fn trailing_separators_do_not_hide_overlapping_roots() {
    let mut registry = registry_with_brain_and_family();
    registry.workspaces.get_mut(&name("brain")).unwrap().root = PathBuf::from("/workspaces/brain/");
    registry.workspaces.get_mut(&name("family")).unwrap().root =
        PathBuf::from("/workspaces/brain/family/");

    assert!(matches!(
        validate_registry(&registry),
        Err(RegistryError::OverlappingRoots { first, second })
            if first == Path::new("/workspaces/brain")
                && second == Path::new("/workspaces/brain/family")
    ));
}

#[test]
fn lexical_parent_components_cannot_escape_above_root() {
    let mut registry = registry_with_brain_and_family();
    registry.workspaces.get_mut(&name("family")).unwrap().root =
        PathBuf::from("/../../workspaces/brain/family");

    assert!(matches!(
        validate_registry(&registry),
        Err(RegistryError::OverlappingRoots { first, second })
            if first == Path::new("/workspaces/brain")
                && second == Path::new("/workspaces/brain/family")
    ));
}
