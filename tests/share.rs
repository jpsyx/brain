use brain::share::{expiration, resolve_path};
use std::path::Path;

#[test]
fn duration_defaults_and_flexible_units() {
    for text in ["", "7days", "7 days", "1 week", "168h"] {
        assert_eq!(expiration(text).unwrap().as_secs(), 604_800, "{text}");
    }
    for text in ["1 hour", "1 hours", "1hr", "1 hr", "1h", "1 h", "1 HOUR"] {
        assert_eq!(expiration(text).unwrap().as_secs(), 3600, "{text}");
    }
    assert_eq!(expiration("30 minutes").unwrap().as_secs(), 1800);
    assert_eq!(expiration("90 sec").unwrap().as_secs(), 90);
    for text in ["1month", "2 months", "1 mo", "1year", "1 yr", "8 days"] {
        assert!(
            expiration(text).unwrap_err().to_string().contains("7 days"),
            "{text}"
        );
    }
    for text in [
        "0h",
        "-1h",
        "7",
        "forever",
        "1 elephant",
        "9999999999999999999999999y",
    ] {
        assert!(expiration(text).is_err(), "{text}");
    }
}

#[test]
fn relative_paths_use_cwd_and_must_belong_to_selected_workspace() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("brain");
    let cwd = root.join("resources");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(cwd.join("paper.pdf"), b"pdf").unwrap();
    let expected = cwd.join("paper.pdf").canonicalize().unwrap();
    assert_eq!(
        resolve_path(Path::new("paper.pdf"), &cwd, &root).unwrap(),
        expected
    );
    assert_eq!(
        resolve_path(&expected, temporary.path(), &root).unwrap(),
        expected
    );
    let error = resolve_path(Path::new("missing"), &cwd, &root).unwrap_err();
    assert!(error.to_string().contains("not a valid file or directory"));
    std::fs::write(temporary.path().join("outside"), b"private").unwrap();
    assert!(resolve_path(&temporary.path().join("outside"), &cwd, &root).is_err());
}

#[test]
fn cli_accepts_optional_path_and_expiration() {
    for args in [
        vec!["brain", "share"],
        vec!["brain", "share", "./paper.pdf"],
        vec!["brain", "share", "/brain/paper.pdf", "--expires", "1 hours"],
    ] {
        let parsed = brain::cli::try_parse_from(args).unwrap();
        assert!(matches!(parsed.command, Some(brain::cli::Cmd::Share(_))));
    }
}
