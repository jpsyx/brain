
#[test]
fn failed_registry_replacement_never_removes_the_portable_markdown_path() {
    let home = tempfile::tempdir().expect("home fixture");
    let config_home = tempfile::tempdir().expect("config fixture");
    let machine_config = config_home.path().join("brain");
    fs::create_dir_all(machine_config.join("env.json")).expect("block registry destination");
    let portable_config_dir = home.path().join("brain/.config");
    fs::create_dir_all(&portable_config_dir).expect("create portable config");
    let portable_path = portable_config_dir.join("config.json");
    let portable = br#"{"markdown_to_pdf_path":"/portable/bin/markdown-to-pdf"}"#;
    fs::write(&portable_path, portable).expect("write portable config");

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "migration-user",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("run failed migration");
    assert!(!output.status.success(), "blocked registry must fail");

    assert_eq!(
        fs::read(portable_path).expect("portable config remains"),
        portable
    );
}
