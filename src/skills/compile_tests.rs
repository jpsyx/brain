use super::install::{Sources, compile_sources};
use crate::workspace::instructions::{plan_document, render_contract, verified_body};
use std::fs;

#[test]
fn no_customization_compilation_matches_fresh_seed_and_receipt_free_adoption() {
    let compiled = compile_sources(&Sources::default()).unwrap();
    assert_eq!(compiled.workspace_body, render_contract());
    let seeded = plan_document(None, &render_contract(), None).unwrap();
    let package = compiled.compose_package(Some(&seeded.after), None).unwrap();
    assert!(!package.instructions.as_ref().unwrap().changed);
    assert!(compiled.projection.sources.is_empty());
}

#[test]
fn raw_source_digest_changes_when_equivalent_rendering_does_not() {
    let root = tempfile::tempdir().unwrap();
    let extensions = root.path().join(".config/extensions");
    fs::create_dir_all(&extensions).unwrap();
    let sources = Sources {
        extensions_dir: Some(extensions.clone()),
        plugins_dir: None,
    };
    fs::write(
        extensions.join("workspace.md"),
        "[workspace:context]\nGLOBAL\n",
    )
    .unwrap();
    let first = compile_sources(&sources).unwrap();
    let first_plan = first
        .compose_package(None, None)
        .unwrap()
        .instructions
        .unwrap();
    fs::write(
        extensions.join("workspace.md"),
        "[workspace:context]\nGLOBAL\n\n",
    )
    .unwrap();
    let second = compile_sources(&sources).unwrap();
    let second_plan = second
        .compose_package(Some(&first_plan.after), Some(&first_plan.receipt))
        .unwrap()
        .instructions
        .unwrap();
    assert_eq!(
        first_plan.receipt.body_sha256,
        second_plan.receipt.body_sha256
    );
    assert_ne!(
        first_plan.receipt.sources_sha256,
        second_plan.receipt.sources_sha256
    );
    assert_eq!(second_plan.receipt.sources_sha256, second.sources_sha256);
    assert_eq!(
        verified_body(&second_plan.after, &second_plan.receipt).unwrap(),
        second.workspace_body
    );
    assert!(second.workspace_body.contains("GLOBAL"));
    assert_eq!(second.workspace_body.matches("GLOBAL").count(), 1);
    assert!(
        second
            .projection
            .sources
            .contains_key(".config/extensions/workspace.md")
    );
}

#[test]
fn portable_instruction_body_and_source_fingerprint_do_not_depend_on_machine_root() {
    let mut outputs = Vec::new();
    for _ in 0..2 {
        let root = tempfile::tempdir().unwrap();
        let extensions = root.path().join(".config/extensions");
        fs::create_dir_all(&extensions).unwrap();
        fs::write(extensions.join("second-brain.md"), "Legacy global context\n[second-brain:company-context]\n[reference](assets/company.md)\n[second-brain:unknown]\nUNKNOWN GLOBAL\n").unwrap();
        let compiled = compile_sources(&Sources {
            extensions_dir: Some(extensions),
            plugins_dir: None,
        })
        .unwrap();
        assert!(compiled.workspace_body.contains("Legacy global context"));
        assert!(compiled.workspace_body.contains("UNKNOWN GLOBAL"));
        assert!(
            compiled
                .workspace_body
                .contains("[reference](assets/company.md)")
        );
        assert!(
            !compiled
                .workspace_body
                .contains(root.path().to_str().unwrap())
        );
        outputs.push((compiled.workspace_body, compiled.sources_sha256));
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn empty_legacy_context_adds_no_reference_and_unused_projected_content_is_retained() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("agenda.md"), "[agenda:anchors]\nFUTURE\n").unwrap();
    let compiled = compile_sources(&Sources {
        extensions_dir: Some(root.path().to_owned()),
        plugins_dir: None,
    })
    .unwrap();
    assert_eq!(
        compiled.projection.by_skill["agenda"].hooks["agenda:anchors"],
        "FUTURE"
    );
    assert!(!compiled.skills["todo"].contains_key("references/legacy-task-context.md"));
}

#[test]
fn empty_extension_sources_render_like_absence_without_leaving_markers() {
    let root = tempfile::tempdir().unwrap();
    let absent = compile_sources(&Sources::default()).unwrap();
    fs::write(root.path().join("todo.md"), "[todo:anchors]\n\n").unwrap();
    fs::write(root.path().join("workspace.md"), "[workspace:context]\n").unwrap();
    let empty = compile_sources(&Sources {
        extensions_dir: Some(root.path().to_owned()),
        plugins_dir: None,
    })
    .unwrap();
    assert_eq!(absent.workspace_body, empty.workspace_body);
    assert_eq!(absent.skills, empty.skills);
    assert_ne!(absent.sources_sha256, empty.sources_sha256);
}
