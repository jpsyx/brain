use super::*;

fn paths(home: &Path) -> crate::workspace::WorkspacePaths {
    crate::workspace::WorkspacePaths::new(home, crate::workspace::WorkspaceId::new())
}

