#[test]
fn tui_construction_is_owned_at_its_boundary() {
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");

    for entry in walkdir::WalkDir::new(&source_root) {
        let entry = entry.expect("walk TUI source");
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
        {
            continue;
        }

        let source = std::fs::read_to_string(path).expect("read TUI source");
        assert!(
            !source.contains("App<'"),
            "{} gives App a lifetime parameter",
            path.display()
        );
        assert!(
            !source.contains("&Cli"),
            "{} stores or accepts the clap task CLI",
            path.display()
        );
        assert!(
            !source.contains("with_receiver"),
            "{} carries the obsolete receiver launch parameter",
            path.display()
        );
    }

    let launch = std::fs::read_to_string(source_root.join("launch.rs")).expect("read launch DTO");
    assert!(launch.contains("pub(crate) struct TuiLaunch"));

    let setup = std::fs::read_to_string(source_root.join("event_loop/setup/mod.rs"))
        .expect("read TUI setup");
    assert!(setup.contains("fn run_tui(launch: TuiLaunch)"));
}
