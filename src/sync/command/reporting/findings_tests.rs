use super::format_file_findings;
use crate::sync::run::RunOutcome;

fn outcome(exit_ok: bool, transferred: u64, deleted: u64, errors: u64) -> RunOutcome {
    RunOutcome {
        exit_ok,
        transferred,
        deleted,
        errors,
        abort: None,
    }
}

#[test]
fn a_quiet_run_says_so_instead_of_saying_nothing() {
    assert_eq!(
        format_file_findings(&outcome(true, 0, 0, 0)),
        "  found: no files differed between this machine and the remote"
    );
}

#[test]
fn movement_is_reported_with_its_counts() {
    assert_eq!(
        format_file_findings(&outcome(true, 3, 0, 0)),
        "  found: 3 file(s) transferred"
    );
    assert_eq!(
        format_file_findings(&outcome(true, 3, 2, 0)),
        "  found: 3 file(s) transferred, 2 file(s) deleted"
    );
    assert_eq!(
        format_file_findings(&outcome(true, 0, 2, 0)),
        "  found: 2 file(s) deleted"
    );
}

#[test]
fn a_failed_run_names_the_error_count_and_the_verdict() {
    let line = format_file_findings(&outcome(false, 0, 0, 4));
    assert!(line.contains("4 error(s)"), "{line}");
    assert!(line.contains("not clean"), "{line}");
}
