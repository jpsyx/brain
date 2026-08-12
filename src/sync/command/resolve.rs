//! `brain sync resolve <original> [...]`: safe delete of conflict copies once
//! you've merged into the canonical original — the local copies **and** the
//! loser objects rclone left on the remote (see [`super::resolve_remote`]).
//! Deletion only — never runs a sync.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::sync::config::SyncConfig;
use crate::sync::conflicts::{self, ConflictFile};
use crate::sync::remote::build_remote;
use crate::theme::Theme;

use super::resolve_remote::{self, RemoteResolution};

/// What resolving one original should do (pure classification, no fs).
#[derive(Debug, PartialEq, Eq)]
pub enum ResolveDecision {
    /// Delete these copies (canonical exists, copies found).
    Delete(Vec<PathBuf>),
    /// Refuse: the canonical original is missing — merge into it first.
    CanonicalMissing,
    /// Nothing to do: no copies for this original.
    NoCopies,
}

/// Decide what to do for one original.
///
/// `canonical_exists` is injected so this stays pure (no fs reads here). A
/// missing canonical always refuses, even when there are no copies either —
/// a bogus/mistyped original shouldn't silently no-op.
#[must_use]
pub fn resolve_decision(
    original: &Path,
    canonical_exists: bool,
    files: &[ConflictFile],
) -> ResolveDecision {
    if !canonical_exists {
        return ResolveDecision::CanonicalMissing;
    }
    let copies = conflicts::copies_for_original(original, files);
    if copies.is_empty() {
        ResolveDecision::NoCopies
    } else {
        ResolveDecision::Delete(copies)
    }
}

/// Whether the remote-cleanup lane should run: only with a configured remote
/// and an rclone to drive it. Both missing halves degrade to a local-only
/// resolve rather than an error — resolving is still useful offline.
#[must_use]
pub fn should_clean_remote(configured: bool, rclone_present: bool) -> bool {
    configured && rclone_present
}

/// `brain sync resolve <original> [...]`: delete the resolved conflict copies
/// for one or more canonical originals, locally and on the remote.
///
/// Only deletes; never runs a sync. Empty `originals` drops into an
/// interactive picker over the currently open conflict groups.
pub fn resolve(root: &Path, cfg: &SyncConfig, originals: &[String]) -> Result<()> {
    if originals.is_empty() {
        return resolve_interactive(root, cfg);
    }
    let theme = Theme::active();
    let files = conflicts::list_conflicts(root);
    resolve_many(root, cfg, originals, &files, theme);
    Ok(())
}

/// Apply the resolve decision to every original in `originals`, in order.
/// Shared by the explicit-args path and the interactive picker.
fn resolve_many(
    root: &Path,
    cfg: &SyncConfig,
    originals: &[String],
    files: &[ConflictFile],
    theme: Theme,
) {
    let remote = should_clean_remote(cfg.is_configured(), crate::sync::run::rclone_present())
        .then(|| build_remote(cfg));
    if remote.is_some() {
        eprintln!(
            "{}",
            theme.muted("Removing the matching loser objects from the remote too…")
        );
    }
    for original in originals {
        resolve_one(root, original, files, theme, remote.as_ref());
    }
}

/// Whether the remote-cleanup lane applies to this decision.
///
/// Both `Delete` and `NoCopies` qualify: an older brain (or another machine)
/// may have removed the local copy while the remote loser still lingers, and
/// that orphan is exactly what this lane exists to collect. `CanonicalMissing`
/// refuses outright, so nothing is touched on either side.
#[must_use]
pub fn remote_lane_applies(decision: &ResolveDecision) -> bool {
    matches!(
        decision,
        ResolveDecision::Delete(_) | ResolveDecision::NoCopies
    )
}

/// Render the summary for an original that had no local copies. Stays on the
/// original plain message unless the remote lane actually did something.
#[must_use]
pub fn no_copies_summary(
    original: &str,
    remote: Option<&RemoteResolution>,
    theme: Theme,
) -> String {
    let plain = format!("no conflict copies for {original}");
    match remote {
        Some(r) if !r.listed => theme.warning(&format!("{plain} (could not check the remote)")),
        Some(r) if !r.deleted.is_empty() => {
            let n = r.deleted.len();
            let noun = if n == 1 { "object" } else { "objects" };
            format!(
                "{} {} {}",
                theme.success("resolved"),
                theme.value(original),
                theme.muted(&format!("(no local copies, {n} remote {noun})")),
            )
        }
        _ => theme.muted(&plain),
    }
}

/// Delete the remote loser objects for `original`, driving real rclone. Thin
/// shell over the tested [`resolve_remote::resolve_remote_with`]; a failed
/// delete is warned about here and reflected in the returned resolution.
fn clean_remote(
    remote: &crate::sync::remote::Remote,
    original: &Path,
    theme: Theme,
) -> RemoteResolution {
    let mut run = |args: &[String]| crate::sync::run::run_rclone_capture(&remote.env, args);
    let resolution = resolve_remote::resolve_remote_with(&remote.arg, original, &mut run);
    for failed in &resolution.failed {
        eprintln!(
            "{}",
            theme.warning(&format!(
                "could not remove {} from the remote",
                failed.display()
            ))
        );
    }
    resolution
}

/// Render the themed one-line summary for a resolved original.
///
/// `remote` is `None` when the remote lane didn't run (no remote configured, or
/// no rclone), in which case the line stays exactly as the local-only resolve
/// always reported it.
#[must_use]
pub fn resolve_summary(
    original: &str,
    removed_local: usize,
    remote: Option<&RemoteResolution>,
    theme: Theme,
) -> String {
    let word = if removed_local == 1 { "copy" } else { "copies" };
    let mut detail = format!("removed {removed_local} {word}");
    match remote {
        Some(r) if !r.listed => detail.push_str(", could not check the remote"),
        Some(r) if !r.deleted.is_empty() => {
            use std::fmt::Write as _;
            let n = r.deleted.len();
            let noun = if n == 1 { "object" } else { "objects" };
            let _ = write!(detail, ", {n} remote {noun}");
        }
        _ => {}
    }
    format!(
        "{} {} {}",
        theme.success("resolved"),
        theme.value(original),
        theme.muted(&format!("({detail})")),
    )
}

/// Apply the resolve decision for one original: delete its copies (best
/// effort), clean the remote losers when a remote is available, and print a
/// themed summary line. Never returns an error — an individual delete failure
/// is noted and resolution continues.
fn resolve_one(
    root: &Path,
    original: &str,
    files: &[ConflictFile],
    theme: Theme,
    remote: Option<&crate::sync::remote::Remote>,
) {
    let rel = Path::new(original);
    let exists = root.join(rel).exists();
    let decision = resolve_decision(rel, exists, files);
    let cleaned = remote
        .filter(|_| remote_lane_applies(&decision))
        .map(|r| clean_remote(r, rel, theme));
    match decision {
        ResolveDecision::Delete(copies) => {
            let mut removed = 0usize;
            for copy in &copies {
                match fs::remove_file(root.join(copy)) {
                    Ok(()) => removed += 1,
                    Err(e) => {
                        eprintln!(
                            "{}",
                            theme.warning(&format!("could not remove {}: {e}", copy.display()))
                        );
                    }
                }
            }
            println!(
                "{}",
                resolve_summary(original, removed, cleaned.as_ref(), theme)
            );
        }
        ResolveDecision::CanonicalMissing => {
            println!(
                "{}",
                theme.warning(&format!(
                    "the canonical file {original} doesn't exist — merge into it before resolving"
                ))
            );
        }
        ResolveDecision::NoCopies => {
            println!("{}", no_copies_summary(original, cleaned.as_ref(), theme));
        }
    }
}

/// Interactive picker for bare `brain sync resolve` (no originals given).
/// Thin shell over `resolve_many`; never unit-tested (drives `/dev/tty`).
fn resolve_interactive(root: &Path, cfg: &SyncConfig) -> Result<()> {
    let theme = Theme::active();
    let files = conflicts::list_conflicts(root);
    let groups = conflicts::group_conflicts(&files);
    if groups.is_empty() {
        println!("{}", theme.muted("no open conflict copies."));
        return Ok(());
    }

    println!("{}", theme.heading("Open conflicts"));
    for (i, g) in groups.iter().enumerate() {
        let word = if g.copies.len() == 1 {
            "copy"
        } else {
            "copies"
        };
        println!(
            "  {} {} {}",
            theme.accent(&format!("{}.", i + 1)),
            theme.value(&g.original.display().to_string()),
            theme.muted(&format!("({} {word})", g.copies.len())),
        );
    }

    let answer = crate::sync::setup::prompt(
        &theme.prompt("Pick a number to resolve (or \"all\", empty to cancel)"),
        "",
    )?;
    let answer = answer.trim();
    if answer.is_empty() {
        println!("{}", theme.muted("cancelled."));
        return Ok(());
    }

    let chosen: Vec<String> = if answer.eq_ignore_ascii_case("all") {
        groups
            .iter()
            .map(|g| g.original.display().to_string())
            .collect()
    } else {
        match answer.parse::<usize>() {
            Ok(n) if n >= 1 && n <= groups.len() => {
                vec![groups[n - 1].original.display().to_string()]
            }
            _ => {
                println!("{}", theme.warning("not a valid choice."));
                return Ok(());
            }
        }
    };

    resolve_many(root, cfg, &chosen, &files, theme);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unconfigured sync block: the remote lane stays off, so the local-side
    /// fs tests below never reach for rclone or the network.
    fn local_only() -> SyncConfig {
        SyncConfig::default()
    }

    fn idea_conflict_files() -> Vec<ConflictFile> {
        vec![
            ConflictFile {
                path: PathBuf::from("idea (conflict mac 2026-07-25).md"),
            },
            ConflictFile {
                path: PathBuf::from("idea (conflict server 2026-07-24).md"),
            },
            ConflictFile {
                path: PathBuf::from("other (conflict mac 2026-07-25).md"),
            },
        ]
    }

    #[test]
    fn the_remote_lane_runs_whenever_the_canonical_exists_even_with_no_local_copies() {
        // The local copy may already be gone (resolved by an older brain that
        // only cleaned the local side), while the remote loser still lingers.
        assert!(remote_lane_applies(&ResolveDecision::NoCopies));
        assert!(remote_lane_applies(&ResolveDecision::Delete(vec![
            PathBuf::from("idea (conflict mac 2026-07-25).md")
        ])));
        assert!(
            !remote_lane_applies(&ResolveDecision::CanonicalMissing),
            "a missing canonical refuses outright; never touch the remote"
        );
    }

    #[test]
    fn no_copies_summary_reports_a_remote_only_cleanup() {
        let remote = RemoteResolution {
            deleted: vec![PathBuf::from("idea.md.__brainconflict__1")],
            failed: vec![],
            listed: true,
        };
        let line = no_copies_summary("idea.md", Some(&remote), Theme::dark(false));
        assert!(line.contains("idea.md"), "{line}");
        assert!(
            line.contains("1 remote object"),
            "a remote-only cleanup must still be reported: {line}"
        );
    }

    #[test]
    fn no_copies_summary_keeps_the_plain_message_when_both_sides_are_clean() {
        let remote = RemoteResolution {
            deleted: vec![],
            failed: vec![],
            listed: true,
        };
        assert_eq!(
            no_copies_summary("idea.md", Some(&remote), Theme::dark(false)),
            "no conflict copies for idea.md"
        );
        assert_eq!(
            no_copies_summary("idea.md", None, Theme::dark(false)),
            "no conflict copies for idea.md"
        );
    }

    #[test]
    fn should_clean_remote_requires_both_a_configured_remote_and_rclone() {
        assert!(should_clean_remote(true, true));
        assert!(!should_clean_remote(false, true));
        assert!(!should_clean_remote(true, false));
    }

    #[test]
    fn summary_reports_the_remote_objects_it_deleted() {
        let remote = RemoteResolution {
            deleted: vec![PathBuf::from("idea.md.__brainconflict__1")],
            failed: vec![],
            listed: true,
        };
        let line = resolve_summary("idea.md", 1, Some(&remote), Theme::dark(false));
        assert!(line.contains("removed 1 copy"), "{line}");
        assert!(
            line.contains("1 remote object"),
            "the remote deletion must be visible in the summary: {line}"
        );
    }

    #[test]
    fn summary_says_the_remote_was_clean_when_nothing_was_there() {
        let remote = RemoteResolution {
            deleted: vec![],
            failed: vec![],
            listed: true,
        };
        let line = resolve_summary("idea.md", 1, Some(&remote), Theme::dark(false));
        assert!(
            !line.contains("remote object"),
            "no remote losers means no remote count to report: {line}"
        );
        assert!(!line.contains("could not"), "{line}");
    }

    #[test]
    fn summary_admits_when_the_remote_could_not_be_checked() {
        let remote = RemoteResolution::default(); // listed: false
        let line = resolve_summary("idea.md", 1, Some(&remote), Theme::dark(false));
        assert!(
            line.contains("could not check the remote"),
            "an unreachable remote must never read as a clean remote: {line}"
        );
    }

    #[test]
    fn summary_stays_local_only_when_the_remote_lane_did_not_run() {
        let line = resolve_summary("idea.md", 2, None, Theme::dark(false));
        assert_eq!(line, "resolved idea.md (removed 2 copies)");
    }

    #[test]
    fn resolve_decision_refuses_when_canonical_is_missing() {
        let files = idea_conflict_files();
        assert_eq!(
            resolve_decision(Path::new("idea.md"), false, &files),
            ResolveDecision::CanonicalMissing
        );
    }

    #[test]
    fn resolve_decision_deletes_copies_when_canonical_present() {
        let files = idea_conflict_files();
        assert_eq!(
            resolve_decision(Path::new("idea.md"), true, &files),
            ResolveDecision::Delete(vec![
                PathBuf::from("idea (conflict mac 2026-07-25).md"),
                PathBuf::from("idea (conflict server 2026-07-24).md"),
            ])
        );
    }

    #[test]
    fn resolve_decision_reports_no_copies_when_canonical_present_but_unmatched() {
        let files = idea_conflict_files();
        assert_eq!(
            resolve_decision(Path::new("nope.md"), true, &files),
            ResolveDecision::NoCopies
        );
    }

    #[test]
    fn resolve_decision_prefers_canonical_missing_over_no_copies() {
        // Guard precedence: a bogus original with no copies AND a missing
        // canonical is still refused, not silently treated as a no-op.
        let files = idea_conflict_files();
        assert_eq!(
            resolve_decision(Path::new("nope.md"), false, &files),
            ResolveDecision::CanonicalMissing
        );
    }

    #[test]
    fn resolve_deletes_the_copy_and_keeps_the_canonical() {
        // Hermetic fs check: real canonical + real conflict copy in a temp
        // dir, no rclone involved.
        let tmp = std::env::temp_dir().join(format!("brain-resolve-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("idea.md"), b"canonical").unwrap();
        fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

        resolve(&tmp, &local_only(), &["idea.md".to_owned()]).unwrap();

        assert!(
            tmp.join("idea.md").exists(),
            "canonical must survive resolve"
        );
        assert!(
            !tmp.join("idea (conflict mac 2026-07-25).md").exists(),
            "the conflict copy must be deleted"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_deletes_copies_for_multiple_originals_in_one_call() {
        let tmp = std::env::temp_dir().join(format!("brain-resolve-many-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("idea.md"), b"merged idea").unwrap();
        fs::write(tmp.join("other.md"), b"merged other").unwrap();
        fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"idea loser").unwrap();
        fs::write(
            tmp.join("other (conflict mac 2026-07-25).md"),
            b"other loser",
        )
        .unwrap();

        resolve(
            &tmp,
            &local_only(),
            &["idea.md".to_owned(), "other.md".to_owned()],
        )
        .unwrap();

        assert!(
            tmp.join("idea.md").exists(),
            "first canonical must survive resolve"
        );
        assert!(
            tmp.join("other.md").exists(),
            "second canonical must survive resolve"
        );
        assert!(
            !tmp.join("idea (conflict mac 2026-07-25).md").exists(),
            "first conflict copy must be deleted"
        );
        assert!(
            !tmp.join("other (conflict mac 2026-07-25).md").exists(),
            "second conflict copy must be deleted"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_deletes_nested_subdir_conflict_copy() {
        let tmp = std::env::temp_dir().join(format!("brain-resolve-nested-{}", std::process::id()));
        let dir = tmp.join("projects");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("idea.md"), b"merged").unwrap();
        fs::write(dir.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

        resolve(&tmp, &local_only(), &["projects/idea.md".to_owned()]).unwrap();

        assert!(
            dir.join("idea.md").exists(),
            "nested canonical must survive resolve"
        );
        assert!(
            !dir.join("idea (conflict mac 2026-07-25).md").exists(),
            "nested conflict copy must be deleted"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_leaves_everything_when_canonical_is_missing() {
        let tmp =
            std::env::temp_dir().join(format!("brain-resolve-missing-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

        resolve(&tmp, &local_only(), &["idea.md".to_owned()]).unwrap();

        assert!(
            tmp.join("idea (conflict mac 2026-07-25).md").exists(),
            "must refuse to delete when the canonical original is missing"
        );

        fs::remove_dir_all(&tmp).ok();
    }
}
