use super::*;

#[test]
fn installer_diagnostics_preserve_safe_target_input_categories() {
    use std::os::unix::fs::PermissionsExt;
    for fault in [
        "core_plugin_override",
        "minimum_version",
        "membership",
        "unreadable_source",
    ] {
        let mut f = Fixture::new();
        let root = f.add("alpha");
        let mut unreadable = None;
        let artifact = match fault {
            "core_plugin_override" => {
                fs::create_dir_all(root.join(".config/plugins/todo")).unwrap();
                fs::write(
                    root.join(".config/plugins/todo/SKILL.md"),
                    "private-source-sentinel",
                )
                .unwrap();
                ".config/plugins/todo"
            }
            "minimum_version" => {
                let path = root.join(".config/workspace.json");
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["minimum_brain_version"] =
                    serde_json::json!("999.0.0-private-source-sentinel");
                fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
                ".config/workspace.json"
            }
            "membership" => {
                fs::write(root.join(".config/users.json"), r#"{"schema_version":1,"users":[{"id":"other-user","name":"private-source-sentinel","phones":[],"emails":[],"response_email":null}]}"#).unwrap();
                ".config/users.json"
            }
            _ => {
                let path = root.join(".config/extensions/todo.md");
                fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
                unreadable = Some(path);
                ".config/extensions"
            }
        };
        let executable = Path::new(env!("CARGO_BIN_EXE_brain"));
        let digest = format!("{:x}", Sha256::digest(fs::read(executable).unwrap()));
        let output = f
            .command()
            .args(["__refresh-workspace-skills", "--target-binary"])
            .arg(executable)
            .args([
                "--target-version",
                env!("CARGO_PKG_VERSION"),
                "--phase",
                "preflight",
                "--expected-target-sha256",
                &digest,
            ])
            .output()
            .unwrap();
        if let Some(path) = unreadable {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(!output.status.success(), "{fault}");
        let text = String::from_utf8_lossy(&output.stderr);
        for expected in ["alpha", fault, artifact, "retry"] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(!text.contains("private-source-sentinel"), "{text}");
        assert!(!root.join(".agents").exists());
    }
}
