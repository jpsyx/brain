use super::{ProjectionDirection, project_extensions};

#[test]
fn legacy_hooks_reach_only_their_semantic_owner() {
    let sources = std::collections::BTreeMap::from([(
        "todo.md".to_owned(),
        "[todo:anchors]\nANCHOR\n[todo:linear]\nTRACKER\n".to_owned(),
    )]);
    let projected = project_extensions(&sources, ProjectionDirection::Current).unwrap();
    assert_eq!(
        projected.by_skill["agenda"].hooks["agenda:anchors"],
        "ANCHOR"
    );
    assert_eq!(projected.by_skill["todo"].hooks["todo:linear"], "TRACKER");
    assert!(
        !projected.by_skill["todo"]
            .hooks
            .contains_key("todo:anchors")
    );
    assert!(projected.legacy_source_patches.is_empty());
}

#[test]
fn every_existing_hook_keeps_its_semantic_phase() {
    let cases = [
        (
            "second-brain",
            "second-brain:company-context",
            "workspace",
            "workspace:context",
        ),
        (
            "second-brain",
            "second-brain:reference-manager",
            "organize",
            "organize:reference-manager",
        ),
        ("todo", "todo:anchors", "agenda", "agenda:anchors"),
        ("todo", "todo:calendar", "agenda", "agenda:calendar"),
        ("todo", "todo:cutoff", "agenda", "agenda:cutoff"),
        (
            "todo",
            "todo:agenda-after-build",
            "agenda",
            "agenda:after-build",
        ),
        ("todo", "todo:linear-backlog", "todo", "todo:linear-backlog"),
        ("todo", "todo:linear", "todo", "todo:linear"),
        (
            "contacts",
            "contacts:fallback",
            "contacts",
            "contacts:fallback",
        ),
        ("triage", "triage:daily-open", "triage", "triage:daily-open"),
        (
            "triage",
            "triage:daily-subagents",
            "triage",
            "triage:daily-subagents",
        ),
        (
            "triage",
            "triage:daily-linear",
            "triage",
            "triage:daily-linear",
        ),
        (
            "triage",
            "triage:daily-merge",
            "triage",
            "triage:daily-merge",
        ),
        (
            "triage",
            "triage:daily-required-outputs",
            "triage",
            "triage:daily-required-outputs",
        ),
        (
            "triage",
            "triage:weekly-inboxes",
            "triage",
            "triage:weekly-inboxes",
        ),
        (
            "triage",
            "triage:weekly-linear",
            "triage",
            "triage:weekly-linear",
        ),
    ];
    for (source, old, owner, new) in cases {
        let sources = std::collections::BTreeMap::from([(
            format!("{source}.md"),
            format!("[{old}]\nKEEP\n"),
        )]);
        let projected = project_extensions(&sources, ProjectionDirection::Current).unwrap();
        let extension = if owner == "workspace" {
            &projected.workspace
        } else {
            &projected.by_skill[owner]
        };
        assert_eq!(extension.hooks[new], "KEEP", "{old}");
        assert_eq!(projected.by_skill.len(), usize::from(owner != "workspace"));
    }
}

fn sources(entries: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    entries
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect()
}

#[test]
fn absent_extensions_are_a_noop() {
    let projected = project_extensions(&sources(&[]), ProjectionDirection::Current).unwrap();
    assert!(projected.by_skill.is_empty());
    assert_eq!(
        projected.workspace,
        crate::skills::extension::Extension::default()
    );
    assert!(projected.legacy_task_context.is_empty());
}

#[test]
fn legacy_ambiguous_context_keeps_its_original_activation_scope() {
    let projected = project_extensions(
        &sources(&[
            (
                "second-brain.md",
                "GLOBAL\n[second-brain:future]\nUNKNOWN\n",
            ),
            ("todo.md", "TASK\n[todo:future]\nMIXED\n"),
            ("contacts.md", "CONTACT\n[contacts:future]\nCONTACT EXTRA\n"),
        ]),
        ProjectionDirection::Current,
    )
    .unwrap();
    assert_eq!(projected.workspace.catch_all, "GLOBAL\n\nUNKNOWN");
    assert_eq!(projected.legacy_task_context, "TASK\n\nMIXED");
    assert_eq!(projected.by_skill["contacts"].catch_all, "CONTACT");
    assert_eq!(
        projected.by_skill["contacts"].hooks["contacts:future"],
        "CONTACT EXTRA"
    );
    assert!(!projected.by_skill.contains_key("second-brain"));
    assert!(!projected.by_skill.contains_key("todo"));
}

#[test]
fn unknown_hooks_stay_in_their_source_skill() {
    let projected = project_extensions(
        &sources(&[(
            "custom.md",
            "[external:step]\nKEEP\n[todo:anchors]\nCUSTOM\n",
        )]),
        ProjectionDirection::Current,
    )
    .unwrap();
    assert_eq!(projected.by_skill["custom"].hooks["external:step"], "KEEP");
    assert_eq!(projected.by_skill["custom"].hooks["todo:anchors"], "CUSTOM");
    assert_eq!(projected.by_skill.len(), 1);
}

#[test]
fn equivalent_aliases_deduplicate_but_distinct_definitions_conflict() {
    let same = sources(&[
        ("todo.md", "[todo:anchors]\nSAME\n"),
        ("agenda.md", "[agenda:anchors]\nSAME\n"),
    ]);
    let projected = project_extensions(&same, ProjectionDirection::Current).unwrap();
    assert_eq!(projected.by_skill["agenda"].hooks.len(), 1);
    let different = sources(&[
        ("todo.md", "[todo:anchors]\nOLD\n"),
        ("agenda.md", "[agenda:anchors]\nNEW\n"),
    ]);
    let error = project_extensions(&different, ProjectionDirection::Current)
        .unwrap_err()
        .to_string();
    assert!(error.contains("agenda:anchors"), "{error}");
    assert!(
        error.contains("todo.md") && error.contains("agenda.md"),
        "{error}"
    );
}

#[test]
fn empty_alias_does_not_replace_nonempty_content() {
    let projected = project_extensions(
        &sources(&[
            ("todo.md", "[todo:anchors]\n"),
            ("agenda.md", "[agenda:anchors]\nKEEP\n"),
        ]),
        ProjectionDirection::Current,
    )
    .unwrap();
    assert_eq!(projected.by_skill["agenda"].hooks["agenda:anchors"], "KEEP");
}

#[test]
fn duplicate_sections_fail_before_the_map_can_overwrite_them() {
    for body in [
        "[todo:anchors]\nA\n[todo:anchors]\nB\n",
        "[todo:anchors]\nSAME\n[todo:anchors]\nSAME\n",
    ] {
        let error =
            project_extensions(&sources(&[("todo.md", body)]), ProjectionDirection::Current)
                .unwrap_err()
                .to_string();
        assert!(
            error.contains("duplicate")
                && error.contains("todo:anchors")
                && error.contains("todo.md"),
            "{error}"
        );
    }
}

#[test]
fn legacy_filenames_accept_canonical_hook_ids() {
    let projected = project_extensions(
        &sources(&[
            ("todo.md", "[agenda:anchors]\nANCHOR\n"),
            (
                "second-brain.md",
                "[workspace:context]\nCONTEXT\n[organize:reference-manager]\nREFERENCE\n",
            ),
        ]),
        ProjectionDirection::Current,
    )
    .unwrap();
    assert_eq!(
        projected.by_skill["agenda"].hooks["agenda:anchors"],
        "ANCHOR"
    );
    assert_eq!(
        projected.by_skill["organize"].hooks["organize:reference-manager"],
        "REFERENCE"
    );
    assert_eq!(projected.workspace.hooks["workspace:context"], "CONTEXT");
    assert!(projected.workspace.catch_all.is_empty());
    assert!(projected.legacy_task_context.is_empty());
}

#[test]
fn projected_content_preserves_internal_line_endings_and_opaque_shell() {
    let raw = "[todo:anchors]\r\n[resource](./assets/anchor.md)\r\nprintf '%s' '../literal'\r\n";
    let projected = project_extensions(
        &sources(&[("nested/todo.md", raw)]),
        ProjectionDirection::Current,
    )
    .unwrap();
    assert_eq!(
        projected.by_skill["agenda"].hooks["agenda:anchors"],
        "[resource](./assets/anchor.md)\r\nprintf '%s' '../literal'"
    );
}

#[test]
fn block_origins_keep_the_original_resource_base_and_raw_source() {
    let raw =
        "intro\r\n[todo:anchors]\r\n[resource](./assets/anchor.md)\r\nprintf '%s' '../literal'\r\n";
    let projected = project_extensions(
        &sources(&[("nested/todo.md", raw)]),
        ProjectionDirection::Current,
    )
    .unwrap();
    assert_eq!(projected.sources["nested/todo.md"], raw.as_bytes());
    let origin = projected
        .origins
        .iter()
        .find(|origin| origin.logical_hook == "agenda:anchors")
        .unwrap();
    assert_eq!(origin.source_path, "nested/todo.md");
    assert_eq!(origin.source_hook.as_deref(), Some("todo:anchors"));
    assert_eq!(
        &projected.sources[&origin.source_path][origin.content_range.clone()],
        b"[resource](./assets/anchor.md)\r\nprintf '%s' '../literal'\r\n"
    );
}

#[test]
fn duplicate_workspace_sections_report_the_relative_source_identity() {
    let error = project_extensions(
        &sources(&[(
            "workspace.md",
            "[workspace:future]\nA\n[workspace:future]\nB\n",
        )]),
        ProjectionDirection::Current,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("workspace.md") && error.contains("duplicate"),
        "{error}"
    );
}

#[test]
fn workspace_unknown_ids_cannot_override_source_owned_known_mappings() {
    for hook in [
        "second-brain:reference-manager",
        "organize:reference-manager",
    ] {
        for context in ["REFERENCE", "GLOBAL"] {
            let input = sources(&[
                ("second-brain.md", &format!("[{hook}]\nREFERENCE\n")),
                ("workspace.md", &format!("[{hook}]\n{context}\n")),
            ]);
            let projected = project_extensions(&input, ProjectionDirection::Current).unwrap();
            assert_eq!(
                projected.by_skill["organize"].hooks["organize:reference-manager"],
                "REFERENCE"
            );
            assert_eq!(projected.workspace.hooks[hook], context);
        }
    }
}
