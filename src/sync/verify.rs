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

/// Classify a completed run.
///
/// `conflicts` is the count of conflict copies handled (renamed) this run;
/// `leftover_markers` is the count of un-renamed conflict markers found after
/// the post-pass. A run that produced any conflict copy is surfaced as
/// `NeedsAttention` even after the markers were renamed cleanly, so a real
/// conflict is never silently reported as clean.
#[must_use]
pub fn classify(run: &RunOutcome, conflicts: usize, leftover_markers: usize) -> Outcome {
    if let Some(kind) = &run.abort {
        let msg = match kind {
            AbortKind::MaxDelete => &format!(
                "sync aborted: would delete more than the --max-delete threshold. If intentional, run `{}`.",
                crate::workspace::suggest("sync repair")
            ),
            AbortKind::CheckAccess => {
                "sync aborted: check-access marker missing. Run `brain sync repair` to recreate the RCLONE_TEST marker and re-establish the baseline."
            }
            AbortKind::PriorListingMissing => {
                "sync aborted: baseline listings missing. Run `brain sync repair` to re-establish the baseline."
            }
            AbortKind::Other => {
                "sync aborted: rclone exited with an error. See `brain sync status`."
            }
        };
        return Outcome::Aborted(msg.to_owned());
    }
    if run.errors > 0 {
        return Outcome::NeedsAttention(format!(
            "{} transfer error(s); re-run `brain sync`.",
            run.errors
        ));
    }
    if leftover_markers > 0 {
        return Outcome::NeedsAttention(format!(
            "{leftover_markers} conflict copy(ies) could not be renamed; see `brain sync conflicts`."
        ));
    }
    if conflicts > 0 {
        return Outcome::NeedsAttention(format!(
            "{conflicts} conflict copy(ies) created; review with `brain sync conflicts`."
        ));
    }
    Outcome::Clean
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_run() -> RunOutcome {
        RunOutcome {
            exit_ok: true,
            transferred: 5,
            deleted: 1,
            errors: 0,
            abort: None,
        }
    }

    #[test]
    fn clean_when_ok_no_errors_no_conflicts_no_leftover_markers() {
        assert_eq!(classify(&ok_run(), 0, 0), Outcome::Clean);
    }

    #[test]
    fn errors_are_needs_attention() {
        let mut r = ok_run();
        r.errors = 2;
        assert!(matches!(classify(&r, 0, 0), Outcome::NeedsAttention(_)));
    }

    #[test]
    fn check_access_abort_points_at_sync_repair() {
        let mut r = ok_run();
        r.exit_ok = false;
        r.abort = Some(AbortKind::CheckAccess);
        let Outcome::Aborted(msg) = classify(&r, 0, 0) else {
            panic!("expected aborted outcome");
        };
        assert!(msg.contains("check-access"), "{msg}");
        assert!(msg.contains("brain sync repair"), "{msg}");
    }

    #[test]
    fn leftover_markers_are_needs_attention() {
        assert!(matches!(
            classify(&ok_run(), 0, 1),
            Outcome::NeedsAttention(_)
        ));
    }

    #[test]
    fn conflicts_created_are_needs_attention_even_with_no_leftover_markers() {
        // A conflict copy was created and renamed cleanly (leftover 0); it must
        // still surface, not be reported as clean.
        match classify(&ok_run(), 1, 0) {
            Outcome::NeedsAttention(m) => assert!(m.contains("conflict copy"), "{m}"),
            other => panic!("expected NeedsAttention, got {other:?}"),
        }
    }

    #[test]
    fn max_delete_abort_is_aborted_with_repair_hint() {
        let r = RunOutcome {
            exit_ok: false,
            transferred: 0,
            deleted: 0,
            errors: 0,
            abort: Some(AbortKind::MaxDelete),
        };
        match classify(&r, 0, 0) {
            Outcome::Aborted(m) => assert!(m.contains("brain sync repair")),
            other => panic!("expected Aborted, got {other:?}"),
        }
    }
}
