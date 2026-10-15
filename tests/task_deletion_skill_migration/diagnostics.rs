use super::*;

#[test]
fn managed_conflicts_expose_safe_actionable_diagnostics_on_every_command_surface() {
    for collision in [false, true] {
        let mut f = Fixture::new();
        let root = f.add("alpha");
        let (artifact, category) = if collision {
            (".config/plugins/todo/SKILL.md", "core_plugin_override")
        } else {
            assert_ok(&f.sync());
            (".agents/skills/todo/SKILL.md", "managed_content_modified")
        };
        let path = root.join(artifact);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "private-source-sentinel").unwrap();
        for args in [
            vec!["workspace", "list"],
            vec!["-w", "alpha", "skills", "sync"],
            vec!["skills", "sync", "--all-workspaces"],
            vec!["skills", "sync", "--all-workspaces", "--json"],
        ] {
            let output = f.run(&args);
            assert!(!output.status.success());
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            for expected in [
                "alpha",
                category,
                if collision {
                    ".config/plugins/todo"
                } else {
                    artifact
                },
                "retry",
            ] {
                assert!(text.contains(expected), "missing {expected}: {text}");
            }
            assert!(!text.contains("private-source-sentinel"), "{text}");
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                "private-source-sentinel"
            );
            if args.contains(&"--json") {
                let doc = document(&output);
                assert_eq!(doc["workspaces"][0]["diagnostic"]["category"], category);
                assert!(
                    doc["workspaces"][0]["diagnostic"]["hint"]
                        .as_str()
                        .unwrap()
                        .contains("retry")
                );
            }
        }
    }
}
