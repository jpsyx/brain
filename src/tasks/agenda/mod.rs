//! Keeping the day's agenda markdown in sync after a task/habit mutation.
//!
//! The agenda (`/tmp/<date>.md`, plus an optional printable PDF) is downstream
//! of the CSVs: the CSVs are the source of truth and the agenda is a snapshot
//! of them. A mutation that only touched the CSVs leaves the snapshot lying,
//! which matters because the user works off it (and prints it) all day.
//!
//! So every mutation runs one deterministic, section-preserving sync:
//!
//! - `MIT` / `Suggested order` / `Cut order` lose the mutated id (or, for a
//!   completed chunk with an unfinished sibling, hand its slot to the next
//!   chunk), with the ordered lists renumbered.
//! - `Today's habits` and `Completed today` are re-derived from the CSVs.
//! - Everything else — title, `**Load:**`, `**Bottom line:**`, any section
//!   this code has never heard of — is reassembled byte-for-byte.
//!
//! Split as usual: the pure decision ([`sync_markdown`] over parsed markdown
//! and CSV rows, plus [`doc`], [`lines`], [`derive`]) is unit-tested without a
//! filesystem; [`io`] is the thin best-effort shell that reads/writes the file
//! and regenerates the PDF.

mod derive;
mod doc;
mod io;
mod lines;
mod sync;

#[cfg(test)]
mod tests;

pub(crate) use io::{Outcome, Targets, resolve_targets, sync_after_mutation, sync_targets};
pub(crate) use sync::{Action, Snapshot, sync_markdown};

/// Section heading prefixes. Matched by prefix rather than exact text so the
/// agenda author's phrasing (`## ❗ Most important`, `## ❗ MITs`, …) still
/// resolves.
const MIT_HEADING: &str = "## ❗";
const SUGGESTED_HEADING: &str = "## Suggested order";
const CUT_HEADING: &str = "## Cut order";
const HABITS_HEADING: &str = "## 🔁";
const COMPLETED_HEADING: &str = "## ✅";
/// The generic caller-content boundary. Optional content appended by whoever
/// built the agenda stays at the bottom, so a re-derived core section that has
/// to be inserted goes *before* this heading.
const APPENDIX_HEADING: &str = "## Appendix <!-- brain:optional-content -->";
