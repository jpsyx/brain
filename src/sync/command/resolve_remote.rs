//! The remote half of `brain sync resolve`: delete the loser objects rclone
//! left on the remote.
//!
//! rclone writes the losing side of a same-file conflict on **both** sides, but
//! `conflicts::rename_markers` only walks the local root, so the remote keeps
//! the raw `<original>.<MARKER><N>` name. Both the raw and friendly patterns
//! are bisync excludes, so a normal sync can neither remove that object nor
//! bring it down: without this lane every resolved conflict would leave an
//! orphan on the remote forever.
//!
//! The rclone shell is injected as a runner closure, so the decision logic here
//! is testable without touching the network.

use std::path::{Path, PathBuf};

use crate::sync::conflicts;

/// What the remote lane did for one canonical original.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RemoteResolution {
    /// Loser objects deleted from the remote.
    pub deleted: Vec<PathBuf>,
    /// Loser objects that were found but could not be deleted.
    pub failed: Vec<PathBuf>,
    /// Whether the listing succeeded. A failed listing means "unknown", not
    /// "nothing there", so the caller can say so instead of implying a clean
    /// remote.
    pub listed: bool,
}

/// The remote directory to list for `original` (its parent, or the remote root).
#[must_use]
pub fn remote_dir_of(original: &Path) -> PathBuf {
    original
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

/// Join a root-relative path onto the remote arg, matching the convention the
/// other remote lanes use (`csv_sync::remote_csv_arg`).
#[must_use]
pub fn remote_path_arg(remote_arg: &str, rel: &Path) -> String {
    let base = remote_arg.trim_end_matches('/');
    let rel = rel.to_string_lossy();
    if rel.is_empty() {
        base.to_owned()
    } else {
        format!("{base}/{rel}")
    }
}

/// The names directly in one remote directory.
///
/// `rclone lsf --files-only <remote>/<dir>`, non-recursive on purpose: a loser
/// always sits beside its original, so there is no reason to list the whole
/// bucket.
#[must_use]
pub fn list_args(remote_arg: &str, dir: &Path) -> Vec<String> {
    vec![
        "lsf".to_owned(),
        "--files-only".to_owned(),
        remote_path_arg(remote_arg, dir),
    ]
}

/// `rclone deletefile <remote>/<rel>`: remove exactly one object.
///
/// `deletefile` rather than `delete`, which would take a directory and recurse.
#[must_use]
pub fn delete_args(remote_arg: &str, rel: &Path) -> Vec<String> {
    vec!["deletefile".to_owned(), remote_path_arg(remote_arg, rel)]
}

/// Turn `rclone lsf` stdout (bare names, one per line) into paths relative to
/// the brain root, by rejoining `dir`.
#[must_use]
pub fn parse_listing(dir: &Path, out: &str) -> Vec<PathBuf> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.ends_with('/'))
        .map(|name| {
            if dir.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                dir.join(name)
            }
        })
        .collect()
}

/// Delete every remote loser copy of `original`, driving rclone through `run`.
///
/// `run` takes argv and returns `(exit_ok, combined_output)` — the shape of
/// [`crate::sync::run::run_rclone_capture`].
pub fn resolve_remote_with(
    remote_arg: &str,
    original: &Path,
    run: &mut impl FnMut(&[String]) -> (bool, String),
) -> RemoteResolution {
    let dir = remote_dir_of(original);
    let (listed, out) = run(&list_args(remote_arg, &dir));
    if !listed {
        return RemoteResolution::default();
    }
    let present = parse_listing(&dir, &out);
    let losers = conflicts::remote_losers_for_original(original, &present);

    let mut resolution = RemoteResolution {
        listed: true,
        ..RemoteResolution::default()
    };
    for loser in losers {
        let (ok, _) = run(&delete_args(remote_arg, &loser));
        if ok {
            resolution.deleted.push(loser);
        } else {
            resolution.failed.push(loser);
        }
    }
    resolution
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::args::CONFLICT_MARKER;

    #[test]
    fn remote_dir_of_returns_the_parent_and_empty_at_the_root() {
        assert_eq!(
            remote_dir_of(Path::new("a/b/idea.md")),
            PathBuf::from("a/b")
        );
        assert_eq!(remote_dir_of(Path::new("idea.md")), PathBuf::new());
    }

    #[test]
    fn remote_path_arg_joins_without_a_trailing_slash_and_omits_an_empty_rel() {
        assert_eq!(
            remote_path_arg("BRAIN:bucket", Path::new("notes/idea.md")),
            "BRAIN:bucket/notes/idea.md"
        );
        assert_eq!(
            remote_path_arg("BRAIN:bucket/", Path::new("")),
            "BRAIN:bucket"
        );
    }

    #[test]
    fn list_args_lists_one_directory_without_recursing() {
        let args = list_args("BRAIN:bucket", Path::new("notes"));
        assert_eq!(args[0], "lsf");
        assert!(args.contains(&"--files-only".to_owned()));
        assert!(
            !args.iter().any(|a| a == "-R" || a == "--recursive"),
            "a loser sits beside its original; never list the whole bucket"
        );
        assert_eq!(args.last().unwrap(), "BRAIN:bucket/notes");
    }

    #[test]
    fn delete_args_target_exactly_one_object() {
        let args = delete_args(
            "BRAIN:bucket",
            Path::new("notes/idea.md.__brainconflict__1"),
        );
        assert_eq!(
            args,
            vec![
                "deletefile".to_owned(),
                "BRAIN:bucket/notes/idea.md.__brainconflict__1".to_owned()
            ],
            "deletefile, not delete: delete would take a directory and recurse"
        );
    }

    #[test]
    fn parse_listing_rejoins_names_onto_the_directory_and_drops_dirs() {
        let out = "idea.md\nidea.md.__brainconflict__1\nsubdir/\n\n";
        assert_eq!(
            parse_listing(Path::new("notes"), out),
            vec![
                PathBuf::from("notes/idea.md"),
                PathBuf::from("notes/idea.md.__brainconflict__1"),
            ]
        );
        assert_eq!(
            parse_listing(Path::new(""), "idea.md\n"),
            vec![PathBuf::from("idea.md")]
        );
    }

    #[test]
    fn deletes_only_the_remote_losers_and_leaves_the_original() {
        let listing = format!("SKILL.md\nSKILL.md.{CONFLICT_MARKER}2\nother.md\n");
        let mut calls: Vec<Vec<String>> = Vec::new();
        let mut run = |args: &[String]| {
            calls.push(args.to_vec());
            if args[0] == "lsf" {
                (true, listing.clone())
            } else {
                (true, String::new())
            }
        };

        let out = resolve_remote_with("BRAIN:b", Path::new("skills/SKILL.md"), &mut run);

        assert!(out.listed);
        assert_eq!(
            out.deleted,
            vec![PathBuf::from(format!("skills/SKILL.md.{CONFLICT_MARKER}2"))]
        );
        assert!(out.failed.is_empty());
        let deletes: Vec<&Vec<String>> = calls.iter().filter(|c| c[0] == "deletefile").collect();
        assert_eq!(deletes.len(), 1, "exactly one object deleted");
        assert!(
            !deletes[0][1].ends_with("/SKILL.md"),
            "the canonical original must never be deleted"
        );
    }

    #[test]
    fn a_failed_listing_reports_unknown_rather_than_a_clean_remote() {
        let mut run = |_: &[String]| (false, String::new());
        let out = resolve_remote_with("BRAIN:b", Path::new("skills/SKILL.md"), &mut run);
        assert!(!out.listed, "a failed listing is unknown, not empty");
        assert!(out.deleted.is_empty() && out.failed.is_empty());
    }

    #[test]
    fn a_failed_delete_is_reported_and_does_not_stop_the_rest() {
        let listing = format!("a.md\na.md.{CONFLICT_MARKER}1\na.md.{CONFLICT_MARKER}2\n");
        let mut run = |args: &[String]| {
            if args[0] == "lsf" {
                (true, listing.clone())
            } else {
                (!args[1].ends_with('1'), String::new())
            }
        };

        let out = resolve_remote_with("BRAIN:b", Path::new("a.md"), &mut run);

        assert_eq!(
            out.failed,
            vec![PathBuf::from(format!("a.md.{CONFLICT_MARKER}1"))]
        );
        assert_eq!(
            out.deleted,
            vec![PathBuf::from(format!("a.md.{CONFLICT_MARKER}2"))],
            "a failed delete must not abort the remaining losers"
        );
    }
}
