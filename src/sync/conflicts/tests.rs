use super::*;

#[test]
fn round_trips_conflict_name_for_a_matrix() {
    for (orig, host, date) in [
        ("notes/idea.md", "mac", "2026-07-25"),
        ("README", "server-01", "2026-01-02"), // extensionless
        ("a/b c/my great note.md", "mac", "2026-12-31"), // spaces in stem + dir
        ("deep/nested/path/file.tar.gz", "mac", "2026-07-25"), // multi-dot ext
    ] {
        let built = conflict_name(Path::new(orig), host, date);
        let parsed = parse_conflict_name(&built).expect("should parse");
        assert_eq!(parsed.original, PathBuf::from(orig));
        assert_eq!(parsed.host, host);
        assert_eq!(parsed.date, date);
    }
}

#[test]
fn rejects_non_conflict_names() {
    assert!(parse_conflict_name(Path::new("notes/idea.md")).is_none());
    // A real title that happens to mention a conflict but isn't the grammar.
    assert!(parse_conflict_name(Path::new("notes/the (conflict) resolution.md")).is_none());
    // rclone's raw marker is not a friendly copy.
    assert!(parse_conflict_name(Path::new(&format!("idea.md.{CONFLICT_MARKER}1"))).is_none());
}

#[test]
fn rejects_malformed_date_inside_the_parens() {
    // Not zero-padded → doesn't match \d{4}-\d{2}-\d{2}.
    assert!(parse_conflict_name(Path::new("idea (conflict mac 2026-7-5).md")).is_none());
    // Letters where digits belong.
    assert!(parse_conflict_name(Path::new("idea (conflict mac 2026-AB-25).md")).is_none());
}

#[test]
fn rejects_empty_host() {
    assert!(parse_conflict_name(Path::new("idea (conflict  2026-07-25).md")).is_none());
}

#[test]
fn rejects_missing_closing_paren() {
    assert!(parse_conflict_name(Path::new("idea (conflict mac 2026-07-25.md")).is_none());
}

#[test]
fn rejects_trailing_content_after_the_close_paren_that_isnt_an_extension() {
    // Non-empty, non-`.`-prefixed content after `)` fails the extension gate.
    assert!(parse_conflict_name(Path::new("idea (conflict mac 2026-07-25)x.md")).is_none());
}

#[test]
fn list_conflicts_finds_friendly_named_copies_relative_to_root() {
    let tmp = std::env::temp_dir().join(format!("brain-listconflicts-{}", std::process::id()));
    let sub = tmp.join("notes");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("idea (conflict mac 2026-07-25).md"), b"x").unwrap();
    std::fs::write(sub.join("normal.md"), b"y").unwrap();

    let found = list_conflicts(&tmp);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].path,
        std::path::PathBuf::from("notes/idea (conflict mac 2026-07-25).md")
    );

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn inserts_conflict_tag_before_extension() {
    assert_eq!(
        conflict_name(Path::new("notes/idea.md"), "mac", "2026-07-25"),
        PathBuf::from("notes/idea (conflict mac 2026-07-25).md")
    );
}

#[test]
fn handles_extensionless_files() {
    assert_eq!(
        conflict_name(Path::new("README"), "mac", "2026-07-25"),
        PathBuf::from("README (conflict mac 2026-07-25)")
    );
}

#[test]
fn rewrites_a_real_marker_path_to_the_friendly_name() {
    // Real rclone format: `<original>.<MARKER><N>` (literal dot + suffix +
    // trailing integer).
    let marker = PathBuf::from(format!("notes/idea.md.{CONFLICT_MARKER}1"));
    assert_eq!(
        friendly_from_marker(&marker, "mac", "2026-07-25"),
        Some(PathBuf::from("notes/idea (conflict mac 2026-07-25).md"))
    );
}

#[test]
fn rewrites_a_multi_digit_marker() {
    let marker = PathBuf::from(format!("notes/idea.md.{CONFLICT_MARKER}12"));
    assert_eq!(
        friendly_from_marker(&marker, "mac", "2026-07-25"),
        Some(PathBuf::from("notes/idea (conflict mac 2026-07-25).md"))
    );
}

#[test]
fn rewrites_an_extensionless_marker() {
    let marker = PathBuf::from(format!("README.{CONFLICT_MARKER}1"));
    assert_eq!(
        friendly_from_marker(&marker, "mac", "2026-07-25"),
        Some(PathBuf::from("README (conflict mac 2026-07-25)"))
    );
}

#[test]
fn non_marker_path_yields_none() {
    assert_eq!(
        friendly_from_marker(Path::new("notes/idea.md"), "mac", "2026-07-25"),
        None
    );
    // marker text without a trailing digit is not a real rclone marker.
    assert_eq!(
        friendly_from_marker(
            Path::new(&format!("notes/idea.md.{CONFLICT_MARKER}")),
            "mac",
            "2026-07-25"
        ),
        None
    );
}

#[test]
fn is_marker_matches_only_the_real_shape() {
    assert!(is_marker(Path::new(&format!("idea.md.{CONFLICT_MARKER}1"))));
    assert!(is_marker(Path::new(&format!("README.{CONFLICT_MARKER}3"))));
    assert!(!is_marker(Path::new("idea.md")));
    assert!(!is_marker(Path::new(&format!("idea.md.{CONFLICT_MARKER}"))));
    assert!(!is_marker(Path::new(&format!("idea.md{CONFLICT_MARKER}1"))));
}

#[test]
fn groups_multiple_copies_of_one_original() {
    let files = vec![
        ConflictFile {
            path: "idea (conflict mac 2026-07-25).md".into(),
        },
        ConflictFile {
            path: "idea (conflict server 2026-07-24).md".into(),
        },
        ConflictFile {
            path: "other (conflict mac 2026-07-25).md".into(),
        },
    ];
    let groups = group_conflicts(&files);
    assert_eq!(groups.len(), 2);
    let idea = groups
        .iter()
        .find(|g| g.original == Path::new("idea.md"))
        .unwrap();
    assert_eq!(idea.copies.len(), 2);
}

#[test]
fn copies_for_original_returns_only_that_originals_copies() {
    let files = vec![
        ConflictFile {
            path: "idea (conflict mac 2026-07-25).md".into(),
        },
        ConflictFile {
            path: "other (conflict mac 2026-07-25).md".into(),
        },
    ];
    let got = copies_for_original(Path::new("idea.md"), &files);
    assert_eq!(
        got,
        vec![PathBuf::from("idea (conflict mac 2026-07-25).md")]
    );
    assert!(copies_for_original(Path::new("missing.md"), &files).is_empty());
}

#[test]
fn group_conflicts_drops_names_that_dont_parse() {
    let files = vec![
        ConflictFile {
            path: "idea (conflict mac 2026-07-25).md".into(),
        },
        ConflictFile {
            path: "notes.md".into(),
        },
    ];
    let groups = group_conflicts(&files);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].original, PathBuf::from("idea.md"));
}

#[test]
fn group_conflicts_is_deterministic_regardless_of_input_order() {
    let files = vec![
        ConflictFile {
            path: "zeta (conflict mac 2026-07-25).md".into(),
        },
        ConflictFile {
            path: "alpha (conflict server 2026-07-24).md".into(),
        },
        ConflictFile {
            path: "alpha (conflict mac 2026-07-25).md".into(),
        },
    ];
    let groups = group_conflicts(&files);
    let originals: Vec<_> = groups.iter().map(|g| g.original.clone()).collect();
    assert_eq!(
        originals,
        vec![PathBuf::from("alpha.md"), PathBuf::from("zeta.md")]
    );
    let alpha_copies: Vec<_> = groups[0].copies.iter().map(|c| c.path.clone()).collect();
    assert_eq!(
        alpha_copies,
        vec![
            PathBuf::from("alpha (conflict mac 2026-07-25).md"),
            PathBuf::from("alpha (conflict server 2026-07-24).md"),
        ]
    );
}

#[test]
fn rename_markers_moves_real_marker_files_to_friendly_names() {
    let tmp = std::env::temp_dir().join(format!("brain-conflicts-{}", std::process::id()));
    let sub = tmp.join("notes");
    std::fs::create_dir_all(&sub).unwrap();
    let marker = sub.join(format!("idea.md.{CONFLICT_MARKER}1"));
    std::fs::write(&marker, b"loser").unwrap();
    let readme = sub.join(format!("README.{CONFLICT_MARKER}1"));
    std::fs::write(&readme, b"loser").unwrap();

    assert_eq!(leftover_markers(&tmp), 2);
    let n = rename_markers(&tmp, "mac", "2026-07-25");
    assert_eq!(n, 2);
    assert_eq!(leftover_markers(&tmp), 0);
    assert!(sub.join("idea (conflict mac 2026-07-25).md").exists());
    assert!(sub.join("README (conflict mac 2026-07-25)").exists());

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn marker_original_recovers_the_original_from_a_raw_marker() {
    assert_eq!(
        marker_original(Path::new(&format!("notes/idea.md.{CONFLICT_MARKER}2"))),
        Some(PathBuf::from("notes/idea.md"))
    );
    // Extensionless originals carry the marker the same way.
    assert_eq!(
        marker_original(Path::new(&format!("README.{CONFLICT_MARKER}1"))),
        Some(PathBuf::from("README"))
    );
    // A plain file is not a marker.
    assert!(marker_original(Path::new("notes/idea.md")).is_none());
    // A friendly copy is not a raw marker.
    assert!(marker_original(Path::new("idea (conflict mac 2026-07-25).md")).is_none());
}

#[test]
fn remote_losers_matches_raw_markers_and_friendly_copies_for_one_original() {
    let remote = vec![
        PathBuf::from("notes/idea.md"),
        PathBuf::from(format!("notes/idea.md.{CONFLICT_MARKER}1")),
        PathBuf::from(format!("notes/idea.md.{CONFLICT_MARKER}2")),
        PathBuf::from("notes/idea (conflict mac 2026-07-25).md"),
        // Same stem, different original — must not match.
        PathBuf::from(format!("notes/idea-two.md.{CONFLICT_MARKER}1")),
        // Same name in a different directory — must not match.
        PathBuf::from(format!("other/idea.md.{CONFLICT_MARKER}1")),
        PathBuf::from("notes/unrelated.md"),
    ];

    assert_eq!(
        remote_losers_for_original(Path::new("notes/idea.md"), &remote),
        vec![
            PathBuf::from("notes/idea (conflict mac 2026-07-25).md"),
            PathBuf::from(format!("notes/idea.md.{CONFLICT_MARKER}1")),
            PathBuf::from(format!("notes/idea.md.{CONFLICT_MARKER}2")),
        ]
    );
}

#[test]
fn remote_losers_never_returns_the_canonical_original_itself() {
    let remote = vec![
        PathBuf::from("notes/idea.md"),
        PathBuf::from("notes/other.md"),
    ];
    assert!(remote_losers_for_original(Path::new("notes/idea.md"), &remote).is_empty());
}
