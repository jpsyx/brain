#[test]
fn conflicts_json_empty_groups_is_empty_array() {
    let groups: Vec<ConflictGroup> = vec![];
    let meta = |_: &Path| CopyMeta {
        modified: None,
        bytes: None,
    };
    let exists = |_: &Path| false;
    let v = conflicts_json(&groups, meta, exists);
    assert_eq!(v, serde_json::json!([]));
}

#[test]
fn conflicts_json_builds_group_with_injected_copy_metadata() {
    let groups = vec![ConflictGroup {
        original: PathBuf::from("resources/ai/idea.md"),
        copies: vec![ParsedCopy {
            path: PathBuf::from("resources/ai/idea (conflict mac 2026-07-25).md"),
            host: "mac".to_owned(),
            date: "2026-07-25".to_owned(),
        }],
    }];
    let meta = |_: &Path| CopyMeta {
        modified: Some("2026-07-25T10:04:11Z".to_owned()),
        bytes: Some(1841),
    };
    let exists = |_: &Path| true;
    let v = conflicts_json(&groups, meta, exists);
    assert_eq!(
        v,
        serde_json::json!([{
            "original": "resources/ai/idea.md",
            "original_exists": true,
            "copies": [{
                "path": "resources/ai/idea (conflict mac 2026-07-25).md",
                "host": "mac",
                "date": "2026-07-25",
                "modified": "2026-07-25T10:04:11Z",
                "bytes": 1841
            }]
        }])
    );
}

#[test]
fn conflicts_json_missing_metadata_serializes_as_null_not_omitted() {
    let groups = vec![ConflictGroup {
        original: PathBuf::from("notes.md"),
        copies: vec![ParsedCopy {
            path: PathBuf::from("notes (conflict mac 2026-07-25).md"),
            host: "mac".to_owned(),
            date: "2026-07-25".to_owned(),
        }],
    }];
    let meta = |_: &Path| CopyMeta {
        modified: None,
        bytes: None,
    };
    let exists = |_: &Path| false;
    let v = conflicts_json(&groups, meta, exists);
    let copy = &v[0]["copies"][0];
    assert!(copy["modified"].is_null(), "{v}");
    assert!(copy["bytes"].is_null(), "{v}");
    assert_eq!(v[0]["original_exists"], false);
}

#[test]
fn conflicts_json_missing_fs_metadata_serializes_as_null() {
    let tmp = std::env::temp_dir().join(format!("brain-conflict-meta-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let groups = vec![ConflictGroup {
        original: PathBuf::from("notes.md"),
        copies: vec![ParsedCopy {
            path: PathBuf::from("notes (conflict mac 2026-07-25).md"),
            host: "mac".to_owned(),
            date: "2026-07-25".to_owned(),
        }],
    }];

    let v = conflicts_json(
        &groups,
        |rel| copy_meta_from_fs(&tmp, rel),
        |rel| tmp.join(rel).exists(),
    );

    let copy = &v[0]["copies"][0];
    assert!(copy["modified"].is_null(), "{v}");
    assert!(copy["bytes"].is_null(), "{v}");
    assert_eq!(v[0]["original_exists"], false);

    std::fs::remove_dir_all(&tmp).ok();
}
