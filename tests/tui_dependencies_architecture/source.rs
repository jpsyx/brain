use std::path::{Path, PathBuf};

pub(super) fn production_tui_sources(root: &Path) -> Vec<PathBuf> {
    let mut sources = walkdir::WalkDir::new(root)
        .into_iter()
        .map(|entry| entry.expect("walk TUI source"))
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("rs"))
        .filter(|path| !is_test_only_source(path, root))
        .collect::<Vec<_>>();
    sources.sort();
    sources
}

fn is_test_only_source(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).expect("TUI source below TUI root");
    relative
        .components()
        .any(|component| component.as_os_str() == "tests")
        || relative.file_stem().and_then(|stem| stem.to_str()) == Some("tests")
        || relative
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.ends_with("_tests"))
}
