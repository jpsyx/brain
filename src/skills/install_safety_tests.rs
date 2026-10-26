use super::{
    install::{Sources, sync},
    layout::Layout,
};
use std::fs;

#[test]
fn sync_checks_owned_bytes_before_render_replacement() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under_root(root.path());
    sync(&layout, &Sources::default()).unwrap();
    let skill = root.path().join(".agents/skills/todo/SKILL.md");
    fs::write(&skill, "my edits").unwrap();
    assert!(sync(&layout, &Sources::default()).is_err());
    assert_eq!(fs::read(skill).unwrap(), b"my edits");
}

#[test]
fn core_plugin_collision_is_rejected_before_any_generated_output() {
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");
    fs::create_dir_all(plugins.join("todo")).unwrap();
    fs::write(plugins.join("todo/SKILL.md"), "plugin source").unwrap();
    let sources = Sources {
        plugins_dir: Some(plugins.clone()),
        extensions_dir: None,
    };
    assert!(sync(&Layout::under_root(root.path()), &sources).is_err());
    assert!(!root.path().join(".agents/skills").exists());
    assert_eq!(
        fs::read(plugins.join("todo/SKILL.md")).unwrap(),
        b"plugin source"
    );
}

#[test]
fn source_read_errors_do_not_prune_plugins_or_drop_extensions() {
    for extensions in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let exts = root.path().join("extensions");
        fs::create_dir_all(plugins.join("mine")).unwrap();
        fs::create_dir_all(&exts).unwrap();
        fs::write(plugins.join("mine/SKILL.md"), "# source").unwrap();
        fs::write(exts.join("mine.md"), "my customization").unwrap();
        let sources = Sources {
            plugins_dir: Some(plugins.clone()),
            extensions_dir: Some(exts.clone()),
        };
        let layout = Layout::under_root(root.path());
        sync(&layout, &sources).unwrap();
        let skill = root.path().join(".agents/skills/mine/SKILL.md");
        let original = fs::read(&skill).unwrap();
        let source = if extensions { exts } else { plugins };
        fs::rename(&source, source.with_extension("retained")).unwrap();
        fs::write(source, "not a source directory").unwrap();
        assert!(
            sync(&layout, &sources).is_err(),
            "source failure must fail closed"
        );
        assert_eq!(fs::read(skill).unwrap(), original);
    }
}

#[test]
fn plugin_reference_symlinks_fail_without_mutating_sources() {
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");
    fs::create_dir_all(plugins.join("mine/references")).unwrap();
    fs::write(plugins.join("mine/SKILL.md"), "# source").unwrap();
    let target = root.path().join("original.md");
    fs::write(&target, "source bytes").unwrap();
    let link = plugins.join("mine/references/guide.md");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let sources = Sources {
        plugins_dir: Some(plugins),
        extensions_dir: None,
    };
    assert!(sync(&Layout::under_root(root.path()), &sources).is_err());
    assert_eq!(fs::read(target).unwrap(), b"source bytes");
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
}

#[test]
fn sandbox_sources_accept_a_relative_root_without_changing_the_process_directory() {
    let root = tempfile::Builder::new()
        .prefix(".skills-test-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let relative = std::path::PathBuf::from(root.path().file_name().unwrap()).join("plugins");
    fs::create_dir_all(relative.join("mine")).unwrap();
    fs::write(relative.join("mine/SKILL.md"), "relative source").unwrap();
    let sources = Sources {
        plugins_dir: Some(relative),
        extensions_dir: None,
    };
    sync(&Layout::under_root(root.path()), &sources).unwrap();
    assert_eq!(
        fs::read(root.path().join(".agents/skills/mine/SKILL.md")).unwrap(),
        b"relative source"
    );
}

#[test]
fn every_split_successor_refuses_user_source_and_destination_collisions_before_publication() {
    for name in [
        "agenda",
        "projects",
        "process-capture",
        "organize",
        "brain-sync",
    ] {
        for parent in [
            ".config/plugins",
            ".agents/skills",
            ".claude/skills",
            ".codex/skills",
            ".opencode/skills",
        ] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join(parent).join(name).join("SKILL.md");
            fs::create_dir_all(source.parent().unwrap()).unwrap();
            fs::write(&source, "user-owned bytes").unwrap();
            let sources = Sources {
                plugins_dir: Some(root.path().join(".config/plugins")),
                extensions_dir: None,
            };
            assert!(
                sync(&Layout::under_root(root.path()), &sources).is_err(),
                "{parent}/{name}"
            );
            assert_eq!(fs::read(&source).unwrap(), b"user-owned bytes");
            assert!(!root.path().join(".agents/skills/todo").exists());
            assert!(!root.path().join("AGENTS.md").exists());
        }
    }
}
