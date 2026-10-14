use super::*;

fn paths(home: &Path) -> crate::workspace::WorkspacePaths {
    crate::workspace::WorkspacePaths::new(home, crate::workspace::WorkspaceId::new())
}

// Existing transport fixtures retain their explicit cache/root layout while
// exercising the acquiring public boundary with a complete test workspace.
fn test_workspace(
    paths: &crate::workspace::WorkspacePaths,
    root: &Path,
) -> crate::workspace::WorkspaceContext {
    let cache = paths.cache_dir();
    let id =
        crate::workspace::WorkspaceId::parse(cache.file_name().unwrap().to_str().unwrap()).unwrap();
    let home = cache.ancestors().nth(4).unwrap();
    crate::workspace::WorkspaceContext::new(
        home,
        id,
        crate::workspace::WorkspaceName::parse("sample").unwrap(),
        root,
        "member",
        home,
    )
    .unwrap()
}

fn sync_csvs_with_transport(
    paths: &crate::workspace::WorkspacePaths,
    root: &Path,
    direction: Direction,
    fetch: impl FnMut(&str) -> Option<String>,
    push: impl FnMut(&str, &str) -> bool,
) -> Result<CsvSyncResult, CsvSyncError> {
    super::sync_csvs_with_transport(&test_workspace(paths, root), direction, fetch, push)
}
