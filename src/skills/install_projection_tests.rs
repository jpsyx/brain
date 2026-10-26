use super::*;

#[test]
fn bundled_render_loads_workspace_guidance_and_packages_mixed_legacy_context() {
    let root = tempfile::tempdir().unwrap();
    let extensions = root.path().join("extensions");
    fs::create_dir(&extensions).unwrap();
    let source = "MIXED task and scheduling preferences\n[todo:anchors]\nANCHOR\n";
    fs::write(extensions.join("todo.md"), source).unwrap();
    let rendered = render_sources(&Sources {
        extensions_dir: Some(extensions.clone()),
        plugins_dir: None,
    })
    .unwrap();
    let text = String::from_utf8_lossy(&rendered["todo"]["SKILL.md"]);
    assert!(
        text.contains("read its `AGENTS.md`"),
        "bundled skill needs workspace prelude"
    );
    assert!(rendered["todo"].contains_key("references/legacy-task-context.md"));
    assert_eq!(
        fs::read(extensions.join("todo.md")).unwrap(),
        source.as_bytes()
    );
}

#[test]
fn duplicate_extension_sections_fail_before_rendering() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("todo.md"),
        "[todo:linear]\nFIRST\n[todo:linear]\nSECOND\n",
    )
    .unwrap();
    assert!(
        render_sources(&Sources {
            extensions_dir: Some(root.path().to_owned()),
            plugins_dir: None,
        })
        .is_err()
    );
}

fn bundled(name: &str, body: &str) -> super::super::model::Skill {
    super::super::model::Skill {
        name: name.to_owned(),
        bundled: true,
        files: vec![super::super::model::SkillFile {
            rel_path: "SKILL.md".into(),
            contents: body.as_bytes().to_vec(),
        }],
    }
}

#[test]
fn duplicate_markers_cannot_run_one_customization_twice() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("agenda.md"),
        "[agenda:anchors]\nSIDE EFFECT\n",
    )
    .unwrap();
    let checked =
        super::super::compatibility::snapshot::CheckedSources::capture(Some(root.path())).unwrap();
    let result = render::compile(
        &[bundled(
            "agenda",
            "<!-- brain:ext agenda:anchors -->\n<!-- brain:ext agenda:anchors -->\n",
        )],
        &checked,
    );
    assert!(result.is_err(), "duplicate placements must fail closed");
}

#[test]
fn ordinary_and_agenda_only_catalogs_share_checked_projection_and_reference() {
    let root = tempfile::tempdir().unwrap();
    let extensions = root.path().join(".config/extensions");
    fs::create_dir_all(&extensions).unwrap();
    let source =
        "MIXED [resource](assets/guide.md)\n[todo:anchors]\nANCHOR\n[todo:unknown]\nUNKNOWN\n";
    fs::write(extensions.join("todo.md"), source).unwrap();
    let checked =
        super::super::compatibility::snapshot::CheckedSources::capture(Some(&extensions)).unwrap();
    let catalog = [
        bundled("todo", "# Todo\n"),
        bundled(
            "agenda",
            "---\nname: agenda\n---\n# Agenda\n<!-- brain:ext agenda:anchors -->\n",
        ),
    ];
    let ordinary = render::compile(&catalog, &checked).unwrap();
    let selected = render::compile(&catalog[1..], &checked).unwrap();
    assert_eq!(ordinary["agenda"], selected["agenda"]);
    assert_eq!(
        ordinary["todo"]["references/legacy-task-context.md"],
        selected["agenda"]["references/legacy-task-context.md"]
    );
    let agenda = String::from_utf8_lossy(&selected["agenda"]["SKILL.md"]);
    assert!(agenda.starts_with("---\nname: agenda\n---\n"));
    assert_eq!(agenda.matches("ANCHOR").count(), 1);
    assert!(!agenda.contains("brain:ext"));
    let reference =
        String::from_utf8_lossy(&selected["agenda"]["references/legacy-task-context.md"]);
    for required in [
        "sha256:",
        "Reuse this already-loaded context",
        "Do not rerun completed side effects",
        "MIXED",
        "UNKNOWN",
        "[resource](assets/guide.md)",
        "`.config/extensions`",
    ] {
        assert!(reference.contains(required), "missing {required}");
    }
    assert_eq!(
        fs::read(extensions.join("todo.md")).unwrap(),
        source.as_bytes()
    );
    assert_eq!(
        checked.projection.sources[".config/extensions/todo.md"],
        source.as_bytes()
    );
}

#[test]
fn effective_hook_uses_nonempty_origin_and_deduplicates_linked_aliases() {
    let link = "[resource](assets/guide.md)";
    for (legacy, canonical, expected_source) in [
        ("", link, "agenda"),
        (link, "", "todo"),
        (link, link, "agenda"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let extensions = root.path().join("extensions");
        fs::create_dir(&extensions).unwrap();
        fs::write(
            extensions.join("todo.md"),
            format!("[todo:anchors]\n{legacy}\n"),
        )
        .unwrap();
        fs::write(
            extensions.join("agenda.md"),
            format!("[agenda:anchors]\n{canonical}\n"),
        )
        .unwrap();
        let checked =
            super::super::compatibility::snapshot::CheckedSources::capture(Some(&extensions))
                .unwrap();
        let output = render::compile(
            &[bundled("agenda", "<!-- brain:ext agenda:anchors -->\n")],
            &checked,
        )
        .unwrap();
        let body = String::from_utf8_lossy(&output["agenda"]["SKILL.md"]);
        assert_eq!(body.matches("[resource](assets/guide.md)").count(), 1);
        assert!(body.contains(&format!(
            "Customization source: `extensions/{expected_source}.md`"
        )));
        assert_eq!(
            checked
                .projection
                .origins
                .iter()
                .filter(|o| o.logical_hook == "agenda:anchors")
                .count(),
            2
        );
    }
}

#[test]
fn conflicting_nonempty_aliases_fail_without_changing_sources() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("todo.md"), "[todo:anchors]\nA\n").unwrap();
    fs::write(root.path().join("agenda.md"), "[agenda:anchors]\nB\n").unwrap();
    assert!(
        super::super::compatibility::snapshot::CheckedSources::capture(Some(root.path())).is_err()
    );
    assert_eq!(
        fs::read(root.path().join("todo.md")).unwrap(),
        b"[todo:anchors]\nA\n"
    );
}

#[test]
fn user_plugin_prose_scripts_and_binary_assets_keep_their_original_bases() {
    let root = tempfile::tempdir().unwrap();
    let plugin = root.path().join("plugins/custom");
    fs::create_dir_all(plugin.join("scripts")).unwrap();
    fs::create_dir_all(plugin.join("assets")).unwrap();
    let body = "---\nname: custom\n---\nSee [asset](assets/data.bin), run `scripts/task.sh`, invoke /todo agenda.\n";
    fs::write(plugin.join("SKILL.md"), body).unwrap();
    fs::write(
        plugin.join("scripts/task.sh"),
        b"#!/bin/sh\n# /second-brain cloud-sync\n",
    )
    .unwrap();
    fs::write(plugin.join("assets/data.bin"), b"\x00\xff\x89").unwrap();
    let output = render_sources(&Sources {
        plugins_dir: Some(root.path().join("plugins")),
        extensions_dir: None,
    })
    .unwrap();
    assert_eq!(output["custom"]["SKILL.md"], body.as_bytes());
    assert_eq!(
        output["custom"]["scripts/task.sh"],
        fs::read(plugin.join("scripts/task.sh")).unwrap()
    );
    assert_eq!(output["custom"]["assets/data.bin"], b"\x00\xff\x89");
    assert_eq!(fs::read(plugin.join("SKILL.md")).unwrap(), body.as_bytes());
}

#[test]
fn plugin_can_repeat_its_own_extension_marker() {
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");
    let extensions = root.path().join("extensions");
    fs::create_dir_all(plugins.join("custom")).unwrap();
    fs::create_dir(&extensions).unwrap();
    let body = "# Custom\n<!-- brain:ext custom:repeat -->\n<!-- brain:ext custom:repeat -->\n";
    fs::write(plugins.join("custom/SKILL.md"), body).unwrap();
    fs::write(extensions.join("custom.md"), "[custom:repeat]\nREPEATED\n").unwrap();
    let output = render_sources(&Sources {
        plugins_dir: Some(plugins.clone()),
        extensions_dir: Some(extensions),
    })
    .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output["custom"]["SKILL.md"])
            .matches("REPEATED")
            .count(),
        2
    );
    assert_eq!(
        fs::read(plugins.join("custom/SKILL.md")).unwrap(),
        body.as_bytes()
    );
}

#[test]
fn sandbox_resource_annotations_resolve_from_both_entrypoint_and_generated_reference() {
    let root = tempfile::tempdir().unwrap();
    let extensions = root.path().join("extensions");
    fs::create_dir_all(extensions.join("assets")).unwrap();
    let resource = extensions.join("assets/guide.md");
    fs::write(&resource, "SOURCE RESOURCE").unwrap();
    let source =
        "Mixed [guide](assets/guide.md)\n[todo:linear-backlog]\nUse [guide](assets/guide.md)\n";
    fs::write(extensions.join("todo.md"), source).unwrap();
    sync(
        &Layout::under_root(root.path()),
        &Sources {
            extensions_dir: Some(extensions.clone()),
            plugins_dir: None,
        },
    )
    .unwrap();
    let owning_skill = root.path().join(".agents/skills/todo");
    for document in ["SKILL.md", "references/legacy-task-context.md"] {
        let body = fs::read_to_string(owning_skill.join(document)).unwrap();
        let base = body
            .split("Resolve its relative resource paths from `")
            .nth(1)
            .unwrap()
            .split('`')
            .next()
            .unwrap();
        let resolved = owning_skill.join(base).join("assets/guide.md");
        assert_eq!(
            fs::canonicalize(&resolved).unwrap(),
            fs::canonicalize(&resource).unwrap(),
            "{document}"
        );
        assert_eq!(fs::read_to_string(&resolved).unwrap(), "SOURCE RESOURCE");
        assert!(
            body.contains("owning installed `SKILL.md` directory"),
            "{document}"
        );
    }
    assert_eq!(
        fs::read(extensions.join("todo.md")).unwrap(),
        source.as_bytes()
    );
}
