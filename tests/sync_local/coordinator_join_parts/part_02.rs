
fn write_remote_legacy(remote: &Path, source: &Path) {
    for relative in [
        ".config/workspace.json",
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "RCLONE_TEST",
        "stable.md",
    ] {
        let destination = remote.join(relative);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::copy(source.join(relative), destination).unwrap();
    }
}

fn write_tasks(root: &Path, text: &str) {
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(root.join("tasks/tasks.csv"), text).unwrap();
}

fn write_registry(config_home: &Path, root: &Path) {
    std::fs::create_dir_all(config_home.join("brain")).unwrap();
    let registry = serde_json::json!({
        "schema_version": 3,
        "default_workspace": "family",
        "workspaces": {
            "family": {
                "workspace_id": WORKSPACE_ID,
                "root": root,
                "aliases": [],
                "local_user_id": "pablo",
                "receiver_enabled": false,
                "env": {
                    "sync": {
                        "enabled": true,
                        "b2_bucket": "fixture",
                        "b2_key_id": "fixture-id",
                        "b2_app_key": "fixture-key",
                        "watch": false,
                        "max_delete_percent": 90
                    }
                }
            }
        }
    });
    std::fs::write(
        config_home.join("brain/env.json"),
        format!("{}\n", serde_json::to_string_pretty(&registry).unwrap()),
    )
    .unwrap();
}

fn write_rclone_shim(path: &Path) {
    std::fs::write(
        path,
        br#"#!/bin/sh
set -eu
map_remote() {
  case "$1" in
    BRAIN:*/*) printf '%s/%s' "$REMOTE_ROOT" "${1#BRAIN:*/}" ;;
    BRAIN:*) printf '%s' "$REMOTE_ROOT" ;;
    *) printf '%s' "$1" ;;
  esac
}
command="$1"
shift
case "$command" in
  version) exec "$REAL_RCLONE" version "$@" ;;
  cat|lsf|mkdir|delete|deletefile)
    target="$(map_remote "$1")"
    shift
    exec "$REAL_RCLONE" "$command" "$target" "$@"
    ;;
  copyto|copy)
    source="$(map_remote "$1")"
    destination="$(map_remote "$2")"
    shift 2
    exec "$REAL_RCLONE" "$command" "$source" "$destination" "$@"
    ;;
  bisync)
    left="$(map_remote "$1")"
    right="$(map_remote "$2")"
    shift 2
    exec "$REAL_RCLONE" bisync "$left" "$right" "$@"
    ;;
  *) exec "$REAL_RCLONE" "$command" "$@" ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
