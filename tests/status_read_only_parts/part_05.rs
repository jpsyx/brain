
fn pid_run_logs_unchanged(
    pid: u32,
    before: &BTreeMap<PathBuf, RunLogEntry>,
    after: &BTreeMap<PathBuf, RunLogEntry>,
) -> bool {
    pid_run_logs(pid, before) == pid_run_logs(pid, after)
}

#[test]
fn pid_log_observer_detects_same_size_reused_pid_log_mutation() {
    let pid = std::process::id();
    let suffix = format!("-{pid}.log");
    let log = tempfile::Builder::new()
        .prefix("2026-08-05T00:00:00.000000000-04:00-")
        .suffix(&suffix)
        .tempfile_in("/tmp")
        .expect("unique reused-PID run log");
    std::fs::write(log.path(), b"before").expect("seed reused-PID run log");
    let before = run_log_snapshot();

    std::fs::write(log.path(), b"after!").expect("mutate reused-PID run log");
    let after = run_log_snapshot();

    assert!(!pid_run_logs_unchanged(pid, &before, &after));
}

fn seed_ready_workspace(home: &Path) {
    let root = home.join("brain");
    std::fs::create_dir_all(root.join(".config")).expect("workspace config directory");
    std::fs::create_dir_all(home.join(".config/brain")).expect("machine config directory");
    std::fs::write(
        root.join(".config/workspace.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            "receiver_ingress_id": "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
            "minimum_brain_version": env!("CARGO_PKG_VERSION")
        }))
        .expect("manifest JSON"),
    )
    .expect("workspace manifest");
    std::fs::write(
        root.join(".config/users.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "users": [{
                "id": "pablo",
                "name": "Pablo",
                "phones": [],
                "emails": [],
                "response_email": null
            }]
        }))
        .expect("users JSON"),
    )
    .expect("portable users");
    std::fs::write(
        home.join(".config/brain/env.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": brain::workspace::REGISTRY_SCHEMA_VERSION,
            "default_workspace": "brain",
            "workspaces": {
                "brain": {
                    "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
                    "root": root,
                    "aliases": [],
                    "local_user_id": "pablo",
                    "receiver_enabled": true,
                    "env": {}
                }
            }
        }))
        .expect("registry JSON"),
    )
    .expect("machine registry");
}

fn publish_live_process_record(home: &Path, generation: &str) {
    let paths = brain::server::lifecycle::ServerPaths::from_home(home);
    std::fs::create_dir_all(paths.directory()).expect("server state directory");
    std::fs::write(
        paths.process_record(),
        serde_json::to_vec(&serde_json::json!({
            "pid": std::process::id(),
            "port": 8787,
            "generation": generation,
            "started_at": "2026-08-05T12:00:00Z"
        }))
        .expect("process record JSON"),
    )
    .expect("process record");
}
