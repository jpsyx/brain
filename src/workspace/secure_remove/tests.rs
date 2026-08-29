use std::os::unix::fs::symlink;

use super::remove_regular_file_beneath;

#[test]
fn cleanup_rejects_a_symlink_above_the_cache_root() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let temporary_root =
        std::fs::canonicalize(temporary.path()).expect("canonical temporary directory");
    let cache_parent = temporary_root.join("workspace-caches");
    let cache_root = cache_parent.join("workspace-id");
    let original_target = cache_root.join("responses/instance.json");
    std::fs::create_dir_all(
        original_target
            .parent()
            .expect("original response directory"),
    )
    .expect("original cache tree");
    std::fs::write(&original_target, "original private artifact")
        .expect("original private artifact");

    let retained_parent = temporary_root.join("workspace-caches-real");
    std::fs::rename(&cache_parent, &retained_parent).expect("retain original cache parent");
    let outside_parent = temporary_root.join("outside");
    let outside_target = outside_parent.join("workspace-id/responses/instance.json");
    std::fs::create_dir_all(outside_target.parent().expect("outside response directory"))
        .expect("outside cache tree");
    std::fs::write(&outside_target, "outside private artifact").expect("outside private artifact");
    symlink(&outside_parent, &cache_parent).expect("replace cache parent with symlink");

    remove_regular_file_beneath(&cache_root, std::path::Path::new("responses/instance.json"))
        .expect_err("cleanup must reject every symlinked ancestor");

    assert!(
        outside_target.exists(),
        "cleanup deleted an outside artifact through a higher ancestor symlink"
    );
}

#[test]
fn cleanup_fails_closed_when_bound_quarantine_recovery_exceeds_the_limit() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let temporary_root =
        std::fs::canonicalize(temporary.path()).expect("canonical temporary directory");
    let cache_root = temporary_root.join("workspace-cache");
    let responses = cache_root.join("responses");
    std::fs::create_dir_all(&responses).expect("response directory");
    let leaf_tag = "52c49d2d-839f-55f7-97bc-d14fcb5f7178";
    let quarantine_prefix = format!(".brain-cleanup-{leaf_tag}-");
    for index in 0..9 {
        let quarantine = responses.join(format!(
            ".brain-cleanup-{leaf_tag}-10000000-0000-4000-8000-{index:012}"
        ));
        std::fs::create_dir(&quarantine).expect("bound quarantine");
        std::fs::write(quarantine.join("artifact"), "quarantined private artifact")
            .expect("quarantined artifact");
    }

    remove_regular_file_beneath(&cache_root, std::path::Path::new("responses/instance.json"))
        .expect_err("too many bound quarantines must retain cleanup authority");

    assert!(
        std::fs::read_dir(&responses)
            .expect("response entries")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&quarantine_prefix)
            })
            .count()
            == 9,
        "bounded discovery silently removed or ignored a matching quarantine"
    );
}

#[test]
fn cleanup_fails_closed_on_a_malformed_bound_quarantine_name() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let temporary_root =
        std::fs::canonicalize(temporary.path()).expect("canonical temporary directory");
    let cache_root = temporary_root.join("workspace-cache");
    let responses = cache_root.join("responses");
    std::fs::create_dir_all(&responses).expect("response directory");
    let malformed =
        responses.join(".brain-cleanup-52c49d2d-839f-55f7-97bc-d14fcb5f7178-not-a-random-uuid");
    std::fs::create_dir(&malformed).expect("malformed bound quarantine");
    std::fs::write(malformed.join("artifact"), "quarantined private artifact")
        .expect("quarantined artifact");

    remove_regular_file_beneath(&cache_root, std::path::Path::new("responses/instance.json"))
        .expect_err("malformed bound quarantine must retain cleanup authority");

    assert!(
        malformed.join("artifact").exists(),
        "malformed quarantine cleanup deleted private data without ownership proof"
    );
}
