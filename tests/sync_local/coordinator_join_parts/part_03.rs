
fn find_rclone() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("rclone"))
        .find(|candidate| candidate.is_file())
        .expect("rclone path")
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse(WORKSPACE_ID).unwrap()
}

fn task_rows(root: &Path) -> BTreeMap<String, BTreeMap<String, String>> {
    rows(root, "tasks/tasks.csv")
}

fn habit_rows(root: &Path) -> BTreeMap<String, BTreeMap<String, String>> {
    rows(root, "tasks/habits.csv")
}
