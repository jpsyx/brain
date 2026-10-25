use super::{ProjectionDirection, project_extensions};
use std::collections::BTreeMap;

fn sources(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect()
}

#[test]
fn legacy_only_down_keeps_sources_unchanged_for_both_supported_targets() {
    let input = sources(&[
        ("todo.md", "mixed\r\n[todo:anchors]\r\nANCHOR\r\n"),
        (
            "second-brain.md",
            "global\n[second-brain:company-context]\nCONTEXT\n",
        ),
    ]);
    for target in [
        ProjectionDirection::Legacy105,
        ProjectionDirection::Legacy101,
    ] {
        let projected = project_extensions(&input, target).unwrap();
        assert!(projected.legacy_source_patches.is_empty());
        assert_eq!(projected.by_skill["todo"].hooks["todo:anchors"], "ANCHOR");
        assert_eq!(
            projected.by_skill["second-brain"].hooks["second-brain:company-context"],
            "CONTEXT"
        );
    }
}

#[test]
fn canonical_down_aggregates_one_patch_per_legacy_destination() {
    let original = "intro\r\n[todo:linear]\r\nTRACKER\r\n";
    let input = sources(&[
        ("todo.md", original),
        (
            "agenda.md",
            "[agenda:anchors]\nANCHOR\n[agenda:calendar]\nCALENDAR\n",
        ),
        ("workspace.md", "[workspace:context]\nCONTEXT\n"),
        ("organize.md", "[organize:reference-manager]\nREFERENCE\n"),
    ]);
    for target in [
        ProjectionDirection::Legacy105,
        ProjectionDirection::Legacy101,
    ] {
        let projected = project_extensions(&input, target).unwrap();
        assert_eq!(projected.legacy_source_patches.len(), 2);
        let todo = projected
            .legacy_source_patches
            .iter()
            .find(|patch| patch.relative_path.to_str() == Some("todo.md"))
            .unwrap();
        assert_eq!(todo.before.as_deref(), Some(original.as_bytes()));
        assert!(todo.validate_before(Some(original.as_bytes())).is_ok());
        assert!(todo.validate_before(Some(b"LATER USER EDIT")).is_err());
        assert!(todo.after.starts_with(original.as_bytes()));
        assert_eq!(projected.by_skill["todo"].hooks["todo:anchors"], "ANCHOR");
        assert_eq!(
            projected.by_skill["todo"].hooks["todo:calendar"],
            "CALENDAR"
        );
        assert_eq!(
            projected.by_skill["second-brain"].hooks["second-brain:reference-manager"],
            "REFERENCE"
        );
        assert!(!projected.by_skill.contains_key("agenda"));
        assert_eq!(
            projected.sources["agenda.md"],
            input["agenda.md"].as_bytes()
        );
    }
}

#[test]
fn canonical_unknown_focused_content_refuses_a_broader_legacy_scope() {
    for (path, body) in [
        ("agenda.md", "narrow instructions"),
        ("agenda.md", "[agenda:future]\nNARROW\n"),
        ("projects.md", "project rules"),
    ] {
        let error = project_extensions(&sources(&[(path, body)]), ProjectionDirection::Legacy105)
            .unwrap_err()
            .to_string();
        assert!(error.contains(path), "{error}");
    }
}

#[test]
fn moved_canonical_ids_inside_a_legacy_file_refuse_duplicate_execution() {
    for (path, body) in [
        ("todo.md", "[agenda:anchors]\nANCHOR\n"),
        ("second-brain.md", "[workspace:context]\nCONTEXT\n"),
    ] {
        let input = sources(&[(path, body)]);
        assert!(project_extensions(&input, ProjectionDirection::Current).is_ok());
        let error = project_extensions(&input, ProjectionDirection::Legacy105)
            .unwrap_err()
            .to_string();
        assert!(error.contains(path), "{error}");
    }
}

#[test]
fn workspace_catch_all_and_unknown_hooks_round_trip_as_always_on_context() {
    let mut input = sources(&[
        (
            "workspace.md",
            "GLOBAL\n[workspace:future]\nEXTRA\n[foreign:context]\nFOREIGN\n",
        ),
        ("second-brain.md", "LEGACY\n"),
    ]);
    let down = project_extensions(&input, ProjectionDirection::Legacy105).unwrap();
    assert_eq!(
        down.by_skill["second-brain"].hooks["workspace:catch-all"],
        "GLOBAL"
    );
    assert_eq!(
        down.by_skill["second-brain"].hooks["workspace:future"],
        "EXTRA"
    );
    assert_eq!(
        down.by_skill["second-brain"].hooks["foreign:context"],
        "FOREIGN"
    );
    for patch in down.legacy_source_patches {
        input.insert(
            patch.relative_path.to_str().unwrap().to_owned(),
            String::from_utf8(patch.after).unwrap(),
        );
    }
    let up = project_extensions(&input, ProjectionDirection::Current).unwrap();
    assert_eq!(up.workspace.catch_all, "LEGACY\n\nGLOBAL");
    assert_eq!(up.workspace.hooks["workspace:future"], "EXTRA");
    assert_eq!(up.workspace.hooks["foreign:context"], "FOREIGN");
}

#[test]
fn workspace_catch_all_alias_deduplicates_and_reports_later_conflicts() {
    for (legacy, succeeds) in [("GLOBAL", true), ("EDITED", false)] {
        let input = sources(&[
            ("workspace.md", "GLOBAL\n"),
            (
                "second-brain.md",
                &format!("[workspace:catch-all]\n{legacy}\n"),
            ),
        ]);
        let result = project_extensions(&input, ProjectionDirection::Current);
        if succeeds {
            assert_eq!(result.unwrap().workspace.catch_all, "GLOBAL");
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("workspace:catch-all")
            );
        }
    }
}

#[test]
fn empty_existing_legacy_section_accepts_content_without_duplicate_headers() {
    let input = sources(&[
        ("agenda.md", "[agenda:anchors]\nANCHOR\n"),
        ("todo.md", "[todo:anchors]\n\n[todo:linear]\nTRACKER\n"),
    ]);
    let projected = project_extensions(&input, ProjectionDirection::Legacy105).unwrap();
    let after = String::from_utf8(projected.legacy_source_patches[0].after.clone()).unwrap();
    assert_eq!(after.matches("[todo:anchors]").count(), 1);
    assert_eq!(projected.by_skill["todo"].hooks["todo:anchors"], "ANCHOR");
    assert!(after.contains("[todo:linear]\nTRACKER\n"));
}

#[test]
fn source_patch_paths_cannot_escape_the_extension_source_root() {
    for path in ["../agenda.md", "/tmp/agenda.md"] {
        assert!(
            project_extensions(
                &sources(&[(path, "[agenda:anchors]\nANCHOR\n")]),
                ProjectionDirection::Legacy105
            )
            .is_err()
        );
    }
}

#[test]
fn provenance_authenticates_the_inserted_span_and_rejects_changed_before_images() {
    let input = sources(&[("agenda.md", "[agenda:anchors]\nANCHOR\n")]);
    let down = project_extensions(&input, ProjectionDirection::Legacy105).unwrap();
    let patch = &down.legacy_source_patches[0];
    assert_eq!(patch.provenance.len(), 1);
    let origin = &patch.provenance[0];
    assert_eq!(origin.source_path, "agenda.md");
    assert_eq!(
        origin.source_sha256,
        "380613d81925f5a73cd8d283796a19663566a13ff71c42a8b9096b7138770daf"
    );
    assert_eq!(origin.logical_hook, "agenda:anchors");
    assert_eq!(
        origin.inserted_sha256,
        "575b5e25093dfc2e0b9372afe29a17e14ed36d35544defe738c248760641f638"
    );
    assert_eq!(
        &patch.after[origin.inserted_range.clone()],
        b"[todo:anchors]\nANCHOR\n"
    );
    assert_eq!(&input["agenda.md"][origin.source_range.clone()], "ANCHOR\n");
    assert!(patch.validate_before(None).is_ok());
    assert!(patch.validate_before(Some(b"LATER USER EDIT")).is_err());
    assert!(patch.validate_after(&patch.after).is_ok());
    assert!(patch.validate_after(b"LATER USER EDIT").is_err());
}

#[test]
fn reupgrade_deduplicates_exact_projections_and_preserves_later_edits_as_conflicts() {
    let mut input = sources(&[("agenda.md", "[agenda:anchors]\nANCHOR\n")]);
    let down = project_extensions(&input, ProjectionDirection::Legacy105).unwrap();
    let patch = &down.legacy_source_patches[0];
    input.insert(
        "todo.md".to_owned(),
        String::from_utf8(patch.after.clone()).unwrap(),
    );
    let up = project_extensions(&input, ProjectionDirection::Current).unwrap();
    assert_eq!(up.by_skill["agenda"].hooks.len(), 1);
    assert!(
        project_extensions(&input, ProjectionDirection::Legacy105)
            .unwrap()
            .legacy_source_patches
            .is_empty()
    );
    input.insert(
        "todo.md".to_owned(),
        "[todo:anchors]\nOLDER CLIENT EDIT\n".to_owned(),
    );
    let error = project_extensions(&input, ProjectionDirection::Current)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("agenda:anchors")
            && error.contains("agenda.md")
            && error.contains("todo.md"),
        "{error}"
    );
    assert_eq!(input["todo.md"], "[todo:anchors]\nOLDER CLIENT EDIT\n");
}

#[test]
fn down_refusal_suggests_a_relative_canonical_source_path() {
    let input = sources(&[("todo.md", "[agenda:anchors]\nANCHOR\n")]);
    let error = project_extensions(&input, ProjectionDirection::Legacy105)
        .unwrap_err()
        .to_string();
    assert!(error.contains("move it to agenda.md"), "{error}");
}

#[test]
fn aggregate_spans_remain_valid_when_a_later_block_fills_an_earlier_empty_section() {
    let input = sources(&[
        (
            "agenda.md",
            "[agenda:anchors]\nANCHOR\n[agenda:calendar]\nCALENDAR\n",
        ),
        ("todo.md", "[todo:calendar]\n\n[todo:linear]\nTRACKER\n"),
    ]);
    let down = project_extensions(&input, ProjectionDirection::Legacy105).unwrap();
    let patch = &down.legacy_source_patches[0];
    assert_eq!(patch.provenance.len(), 2);
    assert!(patch.validate_after(&patch.after).is_ok());
    assert_eq!(
        &patch.after[patch.provenance[0].inserted_range.clone()],
        b"\n[todo:anchors]\nANCHOR\n"
    );
    assert_eq!(
        &patch.after[patch.provenance[1].inserted_range.clone()],
        b"CALENDAR\n"
    );
}
