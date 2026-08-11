use super::{find_rollout, rollout_matches};

const ID: &str = "019feb9e-edc0-7252-945a-5e06a30e0eec";

fn rollout_name(id: &str) -> String {
    format!("rollout-2026-08-11T09-49-49-{id}.jsonl")
}

#[test]
fn a_rollout_for_this_session_matches() {
    assert!(rollout_matches(&rollout_name(ID), ID));
}

/// A prefix match would resume a stranger's session, which is worse than
/// failing to resume at all.
#[test]
fn a_longer_id_sharing_our_prefix_never_matches() {
    let longer = format!("{ID}f");
    assert!(!rollout_matches(&rollout_name(&longer), ID));
    assert!(!rollout_matches(&rollout_name(ID), &format!("{ID}f")));
}

#[test]
fn an_id_that_is_not_its_own_trailing_segment_never_matches() {
    // The id must follow a `-`, not merely end the stem.
    assert!(!rollout_matches("rollout-2026-08-11T09-49-49x-abc.jsonl", "x-abc"));
    assert!(rollout_matches("rollout-2026-08-11T09-49-49-x-abc.jsonl", "x-abc"));
}

#[test]
fn unrelated_files_never_match() {
    for name in [
        "history.jsonl",
        "rollout-2026-08-11T09-49-49-other.jsonl",
        "rollout-2026-08-11T09-49-49-019feb9e-edc0-7252-945a-5e06a30e0eec.json",
        "notrollout-2026-08-11T09-49-49-019feb9e-edc0-7252-945a-5e06a30e0eec.jsonl",
    ] {
        assert!(!rollout_matches(name, ID), "{name} must not match");
    }
    assert!(!rollout_matches(&rollout_name(ID), "   "));
}

fn sessions_tree() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (year, month, day) in [("2026", "01", "04"), ("2026", "08", "11")] {
        let directory = root.path().join(year).join(month).join(day);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(rollout_name("stale-session")), b"{}\n").unwrap();
    }
    root
}

#[test]
fn a_session_recorded_on_disk_is_found_through_the_day_tree() {
    let root = sessions_tree();
    let expected = root.path().join("2026/08/11").join(rollout_name(ID));
    std::fs::write(&expected, b"{}\n").unwrap();

    assert_eq!(find_rollout(root.path(), ID), Some(expected));
}

#[test]
fn a_session_recorded_under_an_older_day_is_still_found() {
    let root = sessions_tree();
    let expected = root.path().join("2026/01/04").join(rollout_name(ID));
    std::fs::write(&expected, b"{}\n").unwrap();

    assert_eq!(find_rollout(root.path(), ID), Some(expected));
}

#[test]
fn a_session_with_no_rollout_is_not_resumable() {
    let root = sessions_tree();

    assert_eq!(find_rollout(root.path(), ID), None);
}

#[test]
fn a_missing_sessions_directory_is_not_an_error() {
    let root = tempfile::tempdir().unwrap();

    assert_eq!(find_rollout(&root.path().join("absent"), ID), None);
    assert_eq!(find_rollout(root.path(), ""), None);
}

/// Depth is bounded so a stray deep directory cannot turn one resume check into
/// a full-disk walk.
#[test]
fn the_search_does_not_descend_past_the_day_level() {
    let root = tempfile::tempdir().unwrap();
    let too_deep = root.path().join("2026/08/11/extra/deeper");
    std::fs::create_dir_all(&too_deep).unwrap();
    std::fs::write(too_deep.join(rollout_name(ID)), b"{}\n").unwrap();

    assert_eq!(find_rollout(root.path(), ID), None);
}
