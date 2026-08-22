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

#[test]
fn runtime_startup_builder_has_its_own_focused_module() {
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let runtime =
        std::fs::read_to_string(source_root.join("runtime/mod.rs")).expect("read runtime owner");
    let builder_path = source_root.join("runtime/builder.rs");

    assert!(
        builder_path.is_file(),
        "runtime startup acquisition must live in runtime/builder.rs"
    );
    assert!(runtime.contains("mod builder;"));
    assert!(!runtime.contains("struct RuntimeBuilder"));
    assert!(!runtime.contains("struct PreparedRuntime"));
}

#[test]
fn tui_root_does_not_reexport_panel_side() {
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let root = std::fs::read_to_string(source_root.join("mod.rs")).expect("read TUI root");
    let construct = std::fs::read_to_string(source_root.join("app_state/construct.rs"))
        .expect("read App constructor");

    assert!(!root.contains("use crate::state::PanelSide"));
    assert!(construct.contains("use crate::state::{Db, PanelSide};"));
}
