use std::path::{Path, PathBuf};

use super::{
    SessionDirOverrides, encoded_directory_name, session_directory, session_exists,
    session_file_matches,
};

const SESSION: &str = "0199a1f4-1c0f-7a3b-9d21-6f0f9a0c4e11";

#[test]
fn a_working_directory_encodes_the_way_pi_encodes_it() {
    assert_eq!(
        encoded_directory_name(Path::new("/Users/tester/brain")),
        "--Users-tester-brain--"
    );
    // A space is kept; only separators and a drive colon collapse to `-`.
    assert_eq!(
        encoded_directory_name(Path::new("/Volumes/My Disk/family brain")),
        "--Volumes-My Disk-family brain--"
    );
}

#[test]
fn sessions_live_under_the_home_agent_directory_by_default() {
    assert_eq!(
        session_directory(
            Path::new("/Users/tester/brain"),
            Some(Path::new("/Users/tester")),
            SessionDirOverrides::default(),
        ),
        Some(PathBuf::from(
            "/Users/tester/.pi/agent/sessions/--Users-tester-brain--"
        ))
    );
}

#[test]
fn a_relocated_agent_directory_moves_the_per_workspace_tree_with_it() {
    assert_eq!(
        session_directory(
            Path::new("/Users/tester/brain"),
            Some(Path::new("/Users/tester")),
            SessionDirOverrides {
                agent_dir: Some("~/config/pi"),
                session_dir: None,
            },
        ),
        Some(PathBuf::from(
            "/Users/tester/config/pi/sessions/--Users-tester-brain--"
        ))
    );
}

/// pi stores every session flat in an explicit session directory, so Brain must
/// not append its own per-workspace segment to one.
#[test]
fn an_explicit_session_directory_is_used_flat() {
    assert_eq!(
        session_directory(
            Path::new("/Users/tester/brain"),
            Some(Path::new("/Users/tester")),
            SessionDirOverrides {
                agent_dir: Some("~/config/pi"),
                session_dir: Some("/srv/pi-sessions"),
            },
        ),
        Some(PathBuf::from("/srv/pi-sessions"))
    );
}

#[test]
fn without_a_home_or_an_override_there_is_no_session_directory() {
    assert_eq!(
        session_directory(Path::new("/Users/tester/brain"), None, SessionDirOverrides::default()),
        None
    );
    assert_eq!(
        session_directory(
            Path::new("/Users/tester/brain"),
            None,
            SessionDirOverrides {
                agent_dir: None,
                session_dir: Some("/srv/pi-sessions"),
            },
        ),
        Some(PathBuf::from("/srv/pi-sessions"))
    );
}

#[test]
fn only_the_exact_session_id_matches_a_session_file() {
    assert!(session_file_matches(
        &format!("2026-09-16T09-49-49-000Z_{SESSION}.jsonl"),
        SESSION
    ));
    for other in [
        format!("2026-09-16T09-49-49-000Z_x{SESSION}.jsonl"),
        format!("{SESSION}.jsonl"),
        format!("2026-09-16T09-49-49-000Z_{SESSION}.jsonl.tmp"),
        format!("2026-09-16T09-49-49-000Z_{SESSION}-2.jsonl"),
    ] {
        assert!(!session_file_matches(&other, SESSION), "{other}");
    }
}

#[test]
fn a_recorded_session_is_found_and_a_never_used_one_is_not() {
    let directory = tempfile::tempdir().expect("temporary session directory");
    std::fs::write(
        directory
            .path()
            .join(format!("2026-09-16T09-49-49-000Z_{SESSION}.jsonl")),
        b"{\"type\":\"session\"}\n",
    )
    .expect("write session file");

    assert!(session_exists(directory.path(), SESSION));
    assert!(!session_exists(
        directory.path(),
        "0199a1f4-1c0f-7a3b-9d21-000000000000"
    ));
    assert!(!session_exists(
        &directory.path().join("missing"),
        SESSION
    ));
}
