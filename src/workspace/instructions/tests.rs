use super::document::plan_legacy_document as plan_document;
use super::*;

const LEGACY: &str = include_str!("../../../templates/workspace/legacy/AGENTS-v0.105.3.md");
const TERMINOLOGY: &str = "## Terminology\n\nWhen the user says **\"the brain\"** or **\"my brain\"**, they mean this knowledge\nbase. Treat any such reference as scoped to this workspace root, not to any\nother workspace on the machine.\n\n";
const REGION: &str =
    "<!-- brain:managed-workspace:start schema=1 -->\nCore\n<!-- brain:managed-workspace:end -->\n";

#[test]
fn user_text_outside_owned_region_survives() {
    let old = b"# Personal\nKeep this exactly.\n";
    let first = plan_document(Some(old), "Core one", None).unwrap();
    let next = plan_document(Some(&first.after), "Core two", Some(&first.receipt)).unwrap();
    assert!(next.after.starts_with(old));
    assert_eq!(next.before.as_deref(), Some(first.after.as_slice()));
    assert!(!String::from_utf8(next.after).unwrap().contains("Core one"));
}

#[test]
fn absent_document_keeps_absence_before_image_and_exact_frame() {
    let plan: DocumentPlan = plan_document(None, "Core", None).unwrap();
    assert_eq!(plan.before, None);
    assert_eq!(plan.after, REGION.as_bytes());
    assert!(plan.changed);
    assert!(plan.removed_legacy.is_empty());
}

#[test]
fn identical_plan_and_suffix_edit_keep_region_ownership() {
    let first = plan_document(None, "Core", None).unwrap();
    let suffix = "\n## My rule\nPreserve café exactly.\r\n";
    let edited = [first.after.as_slice(), suffix.as_bytes()].concat();
    let next = plan_document(Some(&edited), "Core", Some(&first.receipt)).unwrap();
    assert!(!next.changed);
    assert_eq!(next.after, edited);
    assert_eq!(next.receipt, first.receipt);
    let changed = plan_document(Some(&edited), "New core", Some(&first.receipt)).unwrap();
    assert!(changed.after.ends_with(suffix.as_bytes()));
}

#[test]
fn appended_region_does_not_change_unterminated_user_bytes() {
    let user = b"User text with no newline";
    let plan = plan_document(Some(user), "Core", None).unwrap();
    assert_eq!(
        plan.after,
        [user.as_slice(), b"\n", REGION.as_bytes()].concat()
    );
    let next = plan_document(Some(&plan.after), "Core", Some(&plan.receipt)).unwrap();
    assert!(!next.changed);
}

#[test]
fn owned_body_hash_excludes_user_text_and_sources_hashes_input() {
    let empty = plan_document(None, "abc", None).unwrap();
    let custom = plan_document(Some(b"User\n"), "abc", None).unwrap();
    assert_eq!(empty.receipt, custom.receipt);
    assert_eq!(
        empty.receipt.body_sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(empty.receipt.sources_sha256, empty.receipt.body_sha256);
}

#[test]
fn missing_and_stale_receipts_adopt_only_complete_desired_render() {
    let old = plan_document(None, "Old core", None).unwrap();
    let mut stale: InstructionReceipt = old.receipt;
    stale.sources_sha256 = "stale sources".to_owned();
    for prior in [None, Some(&stale)] {
        let adopted = plan_document(Some(REGION.as_bytes()), "Core", prior).unwrap();
        assert!(!adopted.changed);
        assert!(plan_document(Some(REGION.as_bytes()), "Different", prior).is_err());
    }
}

#[test]
fn matching_supported_older_receipt_can_update_owned_content() {
    let first = plan_document(None, "Old core", None).unwrap();
    let mut prior = first.receipt;
    prior.generated_revision = 1;
    let next = plan_document(Some(&first.after), "Core", Some(&prior)).unwrap();
    assert_eq!(next.after, REGION.as_bytes());
    assert_eq!(next.receipt.generated_revision, 2);
}

#[test]
fn modified_managed_body_is_a_conflict() {
    let first = plan_document(None, "Core", None).unwrap();
    let edited = REGION.replace("Core", "My edit");
    assert!(plan_document(Some(edited.as_bytes()), "Core", Some(&first.receipt)).is_err());
}

#[test]
fn unsupported_receipt_revisions_are_not_downgraded() {
    let first = plan_document(None, "Core", None).unwrap();
    for (schema_version, generated_revision) in [(1, 4), (2, 2), (0, 2), (1, 0)] {
        let prior = InstructionReceipt {
            schema_version,
            generated_revision,
            ..first.receipt.clone()
        };
        assert!(plan_document(Some(&first.after), "Core", Some(&prior)).is_err());
        assert!(plan_document(None, "Core", Some(&prior)).is_err());
    }
}

#[test]
fn removed_region_and_document_are_recreated() {
    let first = plan_document(None, "Core", None).unwrap();
    for remaining in [None, Some(b"User rules\n".as_slice())] {
        let next = plan_document(remaining, "Core", Some(&first.receipt)).unwrap();
        assert!(next.changed);
        assert_eq!(
            next.after,
            [remaining.unwrap_or_default(), REGION.as_bytes()].concat()
        );
    }
}

#[test]
fn invalid_utf8_is_refused_without_lossy_conversion() {
    assert!(plan_document(Some(b"User\xff"), "Core", None).is_err());
}

#[test]
fn malformed_duplicate_and_unknown_markers_are_conflicts() {
    let cases = [
        REGION.repeat(2),
        REGION.replace("schema=1", "schema=2"),
        REGION.replace("start schema=1", "start"),
        REGION.replace("<!-- brain:managed-workspace:end -->\n", ""),
        REGION.replace("<!-- brain:managed-workspace:start schema=1 -->\n", ""),
        REGION.replace("\nCore\n", "Core\n"),
        REGION.trim_end().to_owned(),
        format!("prefix{REGION}"),
        format!("<!-- brain:managed-workspace:end -->\n{REGION}"),
    ];
    for malformed in cases {
        assert!(
            plan_document(Some(malformed.as_bytes()), "Core", None).is_err(),
            "accepted malformed frame"
        );
    }
    assert!(plan_document(None, REGION, None).is_err());
}

#[test]
fn exact_legacy_template_is_replaced_with_full_removed_span() {
    let plan = plan_document(Some(LEGACY.as_bytes()), "Core", None).unwrap();
    assert_eq!(plan.after, REGION.as_bytes());
    assert_eq!(plan.removed_legacy.len(), 1);
    assert_eq!(plan.removed_legacy[0].range, 0..LEGACY.len());
    assert_eq!(plan.removed_legacy[0].bytes, LEGACY.as_bytes());
}

#[test]
fn unique_complete_legacy_section_is_removed_and_provenance_keeps_offsets() {
    let prefix = "# Personal\nCustom prefix.\n\n";
    let suffix = "## My rule\nUse second-brain for my custom workflow.\n";
    let original = format!("{prefix}{TERMINOLOGY}{suffix}");
    let plan = plan_document(Some(original.as_bytes()), "Core", None).unwrap();
    assert_eq!(plan.after, format!("{prefix}{suffix}{REGION}").as_bytes());
    assert_eq!(plan.removed_legacy.len(), 1);
    assert_eq!(
        plan.removed_legacy[0].range,
        prefix.len()..prefix.len() + TERMINOLOGY.len()
    );
    assert_eq!(plan.removed_legacy[0].bytes, TERMINOLOGY.as_bytes());
    assert_eq!(plan.before.as_deref(), Some(original.as_bytes()));
}

#[test]
fn edited_extended_and_duplicate_legacy_sections_remain_user_owned() {
    for custom in [
        TERMINOLOGY.replace("knowledge", "custom knowledge"),
        format!("{TERMINOLOGY}An extra rule.\n\n"),
        format!("{TERMINOLOGY}{TERMINOLOGY}"),
        "## Terminology\nEntirely user-authored.\n".to_owned(),
    ] {
        let plan = plan_document(Some(custom.as_bytes()), "Core", None).unwrap();
        assert!(plan.after.starts_with(custom.as_bytes()));
        assert!(plan.removed_legacy.is_empty());
    }
}

#[test]
fn legacy_examples_inside_fences_are_not_owned_sections() {
    let quoted = format!("## My examples\n\n```markdown\n{TERMINOLOGY}## Another example\n```\n");
    let plan = plan_document(Some(quoted.as_bytes()), "Core", None).unwrap();
    assert!(plan.removed_legacy.is_empty());
    assert!(plan.after.starts_with(quoted.as_bytes()));
}

#[test]
fn multiple_removed_spans_reconstruct_the_exact_original() {
    let task_start = LEGACY.find("## The task system\n").unwrap();
    let task_end = LEGACY.find("## How to work here\n").unwrap();
    let original = format!(
        "{TERMINOLOGY}## Custom\nKeep this.\n\n{}## Last\nKeep too.\n",
        &LEGACY[task_start..task_end]
    );
    let plan = plan_document(Some(original.as_bytes()), "Core", None).unwrap();
    assert_eq!(plan.removed_legacy.len(), 2);
    let mut restored = plan.after.strip_suffix(REGION.as_bytes()).unwrap().to_vec();
    for span in &plan.removed_legacy {
        restored.splice(
            span.range.start..span.range.start,
            span.bytes.iter().copied(),
        );
    }
    assert_eq!(restored, original.as_bytes());
}

#[test]
fn rendered_contract_uses_central_response_reference_once() {
    let contract = render_contract();
    let reference = crate::response_policy::render_workspace_reference();
    assert_eq!(contract.matches(&reference).count(), 1);
    assert!(!contract.contains("{{"));
    let planned = plan_document(None, &contract, None).unwrap();
    assert!(
        !plan_document(Some(&planned.after), &contract, None)
            .unwrap()
            .changed
    );
}

#[test]
fn rendered_contract_routes_by_capability_and_preserves_native_decisions() {
    let contract = render_contract()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for required in [
        "BRAIN_WORKSPACE_ID",
        "BRAIN_ACTOR_ID",
        "trusted capability",
        "same-named user plugin",
        "new configured namespace",
        "destructive disposition",
        "<namespace>__<outcome>",
        "notes.md",
        "## Summary",
        "## Notes",
        "hard-deadline review before group mutation",
        "managed-triage completion",
        "native project/task/habit commands",
        "recipient",
        "brain clean",
    ] {
        assert!(
            contract.contains(required),
            "missing shared contract: {required}"
        );
    }
    for row in [
        "| `projects` |",
        "| `process-capture` |",
        "| `organize` |",
        "| `brain-sync` |",
        "| `todo` |",
        "| `agenda` |",
        "| `triage` |",
        "| `contacts` |",
        "| `brain-knowledge-capture` |",
        "| `article-summarizer` |",
        "| `brain-skills` |",
    ] {
        assert!(contract.contains(row), "missing route: {row}");
    }
    assert!(!contract.contains("Always use the `second-brain` skill"));
    assert!(!contract.contains("Don't rename or move user-authored files without confirming"));
}

#[test]
fn transaction_verification_reuses_owned_body_without_claiming_user_text() {
    let first = plan_document(None, "Core", None).unwrap();
    let decorated = [
        b"My prefix\n".as_slice(),
        first.after.as_slice(),
        b"My changed suffix",
    ]
    .concat();
    assert_eq!(verified_body(&decorated, &first.receipt).unwrap(), "Core");
    assert!(verified_body(b"Region removed\n", &first.receipt).is_err());
    assert!(verified_body(REGION.replace("Core", "Edited").as_bytes(), &first.receipt).is_err());
    assert!(verified_body(b"\xff", &first.receipt).is_err());
    let mut newer = first.receipt;
    newer.generated_revision = 4;
    assert!(verified_body(&decorated, &newer).is_err());
}

#[test]
fn plain_marker_names_in_user_prose_are_not_delimiters() {
    let text = b"The brain:managed-workspace namespace is reserved.\n";
    let plan = plan_document(Some(text), "Core", None).unwrap();
    assert!(plan.after.starts_with(text));
    assert_eq!(verified_body(&plan.after, &plan.receipt).unwrap(), "Core");
    let malformed = REGION.replace("<!-- brain:", "<!--brain:");
    assert!(plan_document(Some(malformed.as_bytes()), "Core", None).is_err());
}

#[test]
fn exact_legacy_target_contract_surrounded_by_user_text_is_removed_as_one_span() {
    let before = format!("User prefix\n{LEGACY}User suffix\n");
    let plan = plan_document(Some(before.as_bytes()), "Current core", None).unwrap();
    assert_eq!(plan.removed_legacy.len(), 1);
    assert_eq!(plan.removed_legacy[0].bytes, LEGACY.as_bytes());
    assert!(plan.after.starts_with(b"User prefix\nUser suffix\n"));
    assert!(!String::from_utf8_lossy(&plan.after).contains("## Things to avoid"));
}

#[test]
fn duplicate_or_fenced_full_legacy_contracts_are_preserved() {
    for text in [
        format!("{LEGACY}{LEGACY}"),
        format!("```markdown\n{LEGACY}```\n"),
    ] {
        let plan = plan_document(Some(text.as_bytes()), "Current core", None).unwrap();
        assert!(plan.removed_legacy.is_empty());
        assert!(plan.after.starts_with(text.as_bytes()));
    }
}
