use super::*;

#[test]
fn complete_document_replacement_discards_all_inline_customization() {
    let old = format!("# Old instructions\n{}", "Obsolete rule.\n".repeat(2_000));
    let desired = plan_document(None, "Core\nExtension customization.\n", None).unwrap();
    let actual = plan_document(
        Some(old.as_bytes()),
        "Core\nExtension customization.\n",
        None,
    )
    .unwrap();
    assert_eq!(actual.after, desired.after);
    assert_eq!(actual.before.as_deref(), Some(old.as_bytes()));
}

#[test]
fn replacement_cleans_previously_managed_prefix_and_suffix() {
    let old = plan_document(None, "Old core", None).unwrap();
    let mixed = [b"Old prefix\n", old.after.as_slice(), b"Old suffix\n"].concat();
    let actual = plan_document(Some(&mixed), "New core", Some(&old.receipt)).unwrap();
    assert_eq!(
        actual.after,
        plan_document(None, "New core", None).unwrap().after
    );
}

#[test]
fn direct_document_edits_are_repaired_from_sources() {
    let original = plan_document(None, "Core", None).unwrap();
    for edited in [
        b"Entirely overwritten".as_slice(),
        b"<!-- brain:managed-workspace:start schema=99 -->\nEdited core\n",
        b"Invalid UTF-8: \xff",
    ] {
        let repaired = plan_document(Some(edited), "Core", Some(&original.receipt)).unwrap();
        assert_eq!(repaired.after, original.after);
    }
}
