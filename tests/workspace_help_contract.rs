#[test]
fn readme_teaches_workspace_registration_instead_of_a_writable_root_env() {
    let readme = include_str!("../README.md");

    assert!(!readme.contains("brain env set root"), "{readme}");
    assert!(readme.contains("brain workspace create"), "{readme}");
    assert!(readme.contains("brain workspace attach"), "{readme}");
    assert!(readme.contains("--workspace"), "{readme}");
    assert!(!readme.contains("user registry travel"), "{readme}");
}
