//! Post-sync verification: turn a `run::RunOutcome` (+ a leftover-marker count)
//! into a final `Outcome` the journal and CLI report.
//!
//! A run is `Clean` only if rclone exited cleanly with no errors and no
//! un-renamed conflict markers remain; anything else is surfaced.

use crate::sync::run::{AbortKind, RunOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    NeedsAttention(String),
    Aborted(String),
}

impl Outcome {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::NeedsAttention(_) => "needs_attention",
            Self::Aborted(_) => "aborted",
        }
    }
}

/// Classify a completed run. `leftover_markers` is the count of un-renamed
/// conflict markers found after the post-pass.
#[must_use]
pub fn classify(run: &RunOutcome, leftover_markers: usize) -> Outcome {
    if let Some(kind) = &run.abort {
        let msg = match kind {
            AbortKind::MaxDelete => "sync aborted: would delete more than the --max-delete threshold. If intentional, run `brain sync --resync`.",
            AbortKind::PriorListingMissing => "sync aborted: baseline listings missing. Run `brain sync init` to re-establish the baseline.",
            AbortKind::Other => "sync aborted: rclone exited with an error. See `brain sync status`.",
        };
        return Outcome::Aborted(msg.to_owned());
    }
    if run.errors > 0 {
        return Outcome::NeedsAttention(format!("{} transfer error(s); re-run `brain sync`.", run.errors));
    }
    if leftover_markers > 0 {
        return Outcome::NeedsAttention(format!("{leftover_markers} conflict copy(ies) could not be renamed; see `brain sync conflicts`."));
    }
    Outcome::Clean
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_run() -> RunOutcome {
        RunOutcome { exit_ok: true, transferred: 5, deleted: 1, errors: 0, abort: None }
    }

    #[test]
    fn clean_when_ok_no_errors_no_leftover_markers() {
        assert_eq!(classify(&ok_run(), 0), Outcome::Clean);
    }

    #[test]
    fn errors_are_needs_attention() {
        let mut r = ok_run();
        r.errors = 2;
        assert!(matches!(classify(&r, 0), Outcome::NeedsAttention(_)));
    }

    #[test]
    fn leftover_markers_are_needs_attention() {
        assert!(matches!(classify(&ok_run(), 1), Outcome::NeedsAttention(_)));
    }

    #[test]
    fn max_delete_abort_is_aborted_with_resync_hint() {
        let r = RunOutcome { exit_ok: false, transferred: 0, deleted: 0, errors: 0, abort: Some(AbortKind::MaxDelete) };
        match classify(&r, 0) {
            Outcome::Aborted(m) => assert!(m.contains("--resync")),
            other => panic!("expected Aborted, got {other:?}"),
        }
    }
}
