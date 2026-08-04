//! `brain check`: a read-only, themed report of pending sync changes (what a
//! `brain sync` would push/pull), built on a dry-run `rclone bisync`.

use std::path::Path;

use crate::sync::config::SyncConfig;
use crate::sync::csv_sync::{CSVS, baseline_path, remote_csv_arg};
use crate::sync::progress::{self, Change, Side};
use crate::theme::Theme;

mod csv;

pub use csv::{
    CsvCheckError, CsvPending, CsvSideDiff, collect_csv_pending_with_fetch, csv_pending_from_texts,
    diff_csv_rows, format_csv_check_error,
};

#[derive(Debug, Clone, Copy)]
enum CsvDirection {
    Push,
    Pull,
}

/// Build the themed `brain check` report from the detected changes. Pure.
/// `push` = local changes to upload, `pull` = remote changes to download.
#[must_use]
pub fn format_report(push: &[String], pull: &[String], csv: &[CsvPending], theme: Theme) -> String {
    let push_count = pending_count(push, csv, CsvDirection::Push);
    let pull_count = pending_count(pull, csv, CsvDirection::Pull);
    let unchecked = unchecked_remote_csvs(csv);
    if push_count == 0 && pull_count == 0 && unchecked.is_empty() {
        return theme.success("✓ In sync — nothing to push or pull.");
    }

    let mut lines = Vec::new();
    for (label, side, count, direction) in [
        ("push", push, push_count, CsvDirection::Push),
        ("pull", pull, pull_count, CsvDirection::Pull),
    ] {
        if count == 0 {
            continue;
        }
        lines.push(theme.heading(&format!("Changes to {label} ({count}):")));
        for summary in progress::summarize(side) {
            lines.push(format!("  {}", theme.value(&summary)));
        }
        for summary in csv_summaries(csv, direction) {
            lines.push(format!("  {}", theme.value(&summary)));
        }
    }

    if !unchecked.is_empty() {
        lines.push(theme.warning(&format!(
            "Could not check remote CSV changes for {}.",
            unchecked.join(", ")
        )));
    }

    if push_count == 0 && pull_count == 0 {
        return lines.join("\n");
    }

    if has_csv_deltas(csv) {
        lines.push(String::new());
        lines.push(theme.muted(
            "CSV rows are compared against this machine's cached baseline; pull rows are not proof that another machine made the change. brain sync will merge tasks.csv/habits.csv by id.",
        ));
    }

    let brain_sync = theme.accent("brain sync");
    let suggestion = match (push_count == 0, pull_count == 0) {
        (false, false) => format!("Run `{brain_sync}` to push and pull all changes."),
        (false, true) => format!("Run `{brain_sync}` to push your changes."),
        (true, false) => format!("Run `{brain_sync}` to pull the latest changes."),
        (true, true) => unreachable!("early-returned above when both sides are empty"),
    };
    lines.push(String::new());
    lines.push(suggestion);

    lines.join("\n")
}

fn pending_count(files: &[String], csv: &[CsvPending], direction: CsvDirection) -> usize {
    files.len()
        + csv
            .iter()
            .filter_map(|pending| csv_diff(pending, direction))
            .map(CsvSideDiff::total)
            .sum::<usize>()
}

fn csv_diff(csv: &CsvPending, direction: CsvDirection) -> Option<CsvSideDiff> {
    match direction {
        CsvDirection::Push => Some(csv.push),
        CsvDirection::Pull => csv.pull,
    }
}

fn csv_summaries(csv: &[CsvPending], direction: CsvDirection) -> Vec<String> {
    csv.iter()
        .filter_map(|pending| {
            let diff = csv_diff(pending, direction)?;
            (!diff.is_empty()).then(|| {
                format!(
                    "{}: +{} ~{} -{} rows",
                    pending.name, diff.added, diff.changed, diff.deleted
                )
            })
        })
        .collect()
}

fn unchecked_remote_csvs(csv: &[CsvPending]) -> Vec<String> {
    csv.iter()
        .filter(|pending| pending.pull.is_none())
        .map(|pending| pending.name.clone())
        .collect()
}

fn has_csv_deltas(csv: &[CsvPending]) -> bool {
    csv.iter().any(|pending| {
        !pending.push.is_empty() || pending.pull.is_some_and(|diff| !diff.is_empty())
    })
}

fn collect_csv_pending(
    paths: &crate::workspace::WorkspacePaths,
    root: &Path,
    remote_env: &[(String, String)],
    remote_arg: &str,
) -> Result<Vec<CsvPending>, CsvCheckError> {
    crate::logging::log(format!("check csv pending root={}", root.display()));
    collect_csv_pending_with_fetch(
        root,
        &CSVS,
        |name| {
            let path = baseline_path(paths, name);
            crate::logging::log(format!("check csv baseline {}", path.display()));
            match std::fs::read_to_string(path) {
                Ok(text) => Ok(text),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                Err(error) => Err(format!("reading baseline: {error}")),
            }
        },
        |rel| {
            crate::logging::log(format!("check csv remote {rel}"));
            fetch_remote_csv(paths, remote_env, remote_arg, rel)
        },
    )
}

fn fetch_remote_csv(
    paths: &crate::workspace::WorkspacePaths,
    remote_env: &[(String, String)],
    remote_arg: &str,
    rel: &str,
) -> Option<String> {
    let temporary_dir = paths.sync_dir().join("tmp");
    let _ = std::fs::create_dir_all(&temporary_dir);
    let tmp = temporary_dir.join(format!(
        "csv-check-{}-{}",
        std::process::id(),
        rel.replace('/', "_")
    ));
    let args = [
        "copyto".to_owned(),
        remote_csv_arg(remote_arg, rel),
        tmp.to_string_lossy().into_owned(),
    ];
    crate::logging::log(format!(
        "check fetch remote csv rel={rel} tmp={}",
        tmp.display()
    ));
    let (ok, _) = crate::sync::run::run_rclone_capture(remote_env, &args);
    let text = ok.then(|| std::fs::read_to_string(&tmp).ok()).flatten();
    let _ = std::fs::remove_file(&tmp);
    text
}

/// Run `brain check`: dry-run bisync, classify pending changes, print the report.
///
/// Thin IO shell; the report text itself is built by [`format_report`]. Never
/// fails: rclone/IO errors surface as a themed warning rather than a hard
/// error, since this is a read-only convenience report, not a sync.
pub fn run(paths: &crate::workspace::WorkspacePaths, cfg: &SyncConfig, root: &std::path::Path) {
    let theme = Theme::active();
    if !cfg.is_configured() {
        crate::logging::log("check unconfigured");
        println!(
            "{}",
            crate::sync::command::format_unconfigured_sync_guidance(
                crate::sync::args::Direction::Both,
                theme,
            )
        );
        return;
    }
    let remote = crate::sync::remote::build_remote(cfg);
    crate::logging::log(format!(
        "check root={} remote={}",
        root.display(),
        remote.arg
    ));
    let local = root.to_string_lossy().into_owned();
    let workdir = crate::sync::run::bisync_workdir(paths);
    let mut args = crate::sync::args::bisync_args(
        cfg,
        &local,
        &remote.arg,
        &workdir.to_string_lossy(),
        crate::sync::args::Direction::Both,
    );
    args.push("--dry-run".into());
    println!(
        "{}",
        theme.muted("Checking file changes with rclone dry-run…")
    );
    crate::logging::log("check rclone dry-run start");
    let (exit_ok, output) = crate::sync::run::run_rclone_capture(&remote.env, &args);
    crate::logging::log(format!(
        "check rclone dry-run done exit_ok={} output_bytes={}",
        exit_ok,
        output.len()
    ));
    // No baseline yet? bisync aborts with prior-listing-missing.
    if !exit_ok && (output.contains("cannot find prior") || output.contains("Must run --resync")) {
        crate::logging::log("check missing baseline");
        println!(
            "{}",
            theme.warning("No sync baseline yet — run `brain sync` to establish it.")
        );
        return;
    }
    let changes: Vec<Change> = output
        .lines()
        .filter_map(|l| progress::classify_change(&progress::strip(l)))
        .collect();
    let push: Vec<String> = changes
        .iter()
        .filter(|c| c.side == Side::Push)
        .map(|c| c.path.clone())
        .collect();
    let pull: Vec<String> = changes
        .iter()
        .filter(|c| c.side == Side::Pull)
        .map(|c| c.path.clone())
        .collect();
    crate::logging::log(format!(
        "check file changes push={} pull={}",
        push.len(),
        pull.len()
    ));
    println!("{}", theme.muted("Checking task and habit CSV baselines…"));
    match collect_csv_pending(paths, root, &remote.env, &remote.arg) {
        Ok(csv) => {
            crate::logging::log(format!("check csv files={}", csv.len()));
            println!("{}", format_report(&push, &pull, &csv, theme));
        }
        Err(error) => {
            crate::logging::log(format!("check csv failed: {error}"));
            println!("{}", format_csv_check_error(&error, theme));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::csv_merge::SchemaStatus;

    #[test]
    fn in_sync_when_nothing_to_push_or_pull() {
        let t = Theme::dark(false);
        let report = format_report(&[], &[], &[], t);
        assert!(report.contains("In sync"), "{report:?}");
        assert!(!report.contains("brain sync"), "{report:?}");
    }

    #[test]
    fn push_only_reports_count_summary_and_push_suggestion() {
        let t = Theme::dark(false);
        let push = vec!["notes/a.md".to_string(), "notes/b.md".to_string()];
        let report = format_report(&push, &[], &[], t);
        assert!(report.contains("Changes to push (2)"), "{report:?}");
        assert!(report.contains("2 changes in notes/"), "{report:?}");
        assert!(
            report.contains("Run `brain sync` to push your changes."),
            "{report:?}"
        );
    }

    #[test]
    fn pull_only_reports_pull_suggestion() {
        let t = Theme::dark(false);
        let pull = vec!["remote-added.md".to_string()];
        let report = format_report(&[], &pull, &[], t);
        assert!(report.contains("Changes to pull (1)"), "{report:?}");
        assert!(
            report.contains("Run `brain sync` to pull the latest changes."),
            "{report:?}"
        );
    }

    #[test]
    fn both_sides_report_push_and_pull_suggestion() {
        let t = Theme::dark(false);
        let push = vec!["a.md".to_string()];
        let pull = vec!["b.md".to_string()];
        let report = format_report(&push, &pull, &[], t);
        assert!(report.contains("Changes to push (1)"), "{report:?}");
        assert!(report.contains("Changes to pull (1)"), "{report:?}");
        assert!(
            report.contains("Run `brain sync` to push and pull all changes."),
            "{report:?}"
        );
    }

    #[test]
    fn colored_suggestion_wraps_brain_sync_in_accent() {
        let t = Theme::dark(true);
        let push = vec!["a.md".to_string()];
        let report = format_report(&push, &[], &[], t);
        assert!(report.contains("\x1b[96mbrain sync\x1b[0m"), "{report:?}");
    }

    #[test]
    fn csv_side_diff_counts_added_changed_and_deleted_rows() {
        let base = "task_id,title,status\n1,keep,open\n2,change,open\n3,delete,open\n";
        let side = "task_id,title,status\n1,keep,open\n2,changed,open\n4,add,open\n";

        assert_eq!(
            diff_csv_rows(base, side, SchemaStatus::Legacy).unwrap(),
            CsvSideDiff {
                added: 1,
                changed: 1,
                deleted: 1
            }
        );
    }

    #[test]
    fn csv_diff_keys_by_uuid_and_aligns_reordered_headers() {
        let base = "task_uuid,task_id,status,notes\n\
                    10000000-0000-4000-8000-000000000010,T10,open,same\n";
        let side = "notes,status,task_id,task_uuid\n\
                    same,open,T10,10000000-0000-4000-8000-000000000010\n";

        assert_eq!(
            diff_csv_rows(base, side, SchemaStatus::Current).unwrap(),
            CsvSideDiff::default()
        );
    }

    #[test]
    fn malformed_csv_diff_returns_a_typed_error() {
        let error = diff_csv_rows(
            "task_id,notes\nT1,ok\n",
            "task_id,notes\nT1,ok,unexpected\n",
            SchemaStatus::Legacy,
        )
        .unwrap_err();

        assert!(error.to_string().contains("malformed CSV record"));
        assert!(error.to_string().contains("row 2"));
    }

    #[test]
    fn csv_pending_tracks_push_and_pull_sides_independently() {
        let base = "task_id,title,status\n1,base,open\n";
        let local = "task_id,title,status\n1,local,open\n2,local add,open\n";
        let remote = "task_id,title,status\n";

        assert_eq!(
            csv_pending_from_texts(
                "tasks/tasks.csv",
                base,
                local,
                Some(remote),
                SchemaStatus::Legacy,
            )
            .unwrap(),
            CsvPending {
                name: "tasks.csv".to_string(),
                push: CsvSideDiff {
                    added: 1,
                    changed: 1,
                    deleted: 0
                },
                pull: Some(CsvSideDiff {
                    added: 0,
                    changed: 0,
                    deleted: 1
                }),
            }
        );
    }

    #[test]
    fn csv_pending_with_missing_baseline_does_not_double_count_identical_sides() {
        let csv = "task_id,title,status\n1,same,open\n2,also same,open\n";

        assert_eq!(
            csv_pending_from_texts("tasks/tasks.csv", "", csv, Some(csv), SchemaStatus::Legacy,)
                .unwrap(),
            CsvPending {
                name: "tasks.csv".to_string(),
                push: CsvSideDiff::default(),
                pull: Some(CsvSideDiff::default()),
            }
        );
    }

    #[test]
    fn csv_pending_with_missing_baseline_treats_remote_as_provisional_snapshot() {
        let remote = "task_id,title,status\n1,old,open\n";
        let local = "task_id,title,status\n1,old,open\n2,new local,open\n";

        assert_eq!(
            csv_pending_from_texts(
                "tasks/tasks.csv",
                "",
                local,
                Some(remote),
                SchemaStatus::Legacy,
            )
            .unwrap(),
            CsvPending {
                name: "tasks.csv".to_string(),
                push: CsvSideDiff {
                    added: 1,
                    changed: 0,
                    deleted: 0,
                },
                pull: Some(CsvSideDiff::default()),
            }
        );
    }

    #[test]
    fn csv_pending_with_missing_baseline_and_empty_local_reports_pull_only() {
        let remote = "task_id,title,status\n1,remote,open\n";
        let local = "task_id,title,status\n";

        assert_eq!(
            csv_pending_from_texts(
                "tasks/tasks.csv",
                "",
                local,
                Some(remote),
                SchemaStatus::Legacy,
            )
            .unwrap(),
            CsvPending {
                name: "tasks.csv".to_string(),
                push: CsvSideDiff::default(),
                pull: Some(CsvSideDiff {
                    added: 1,
                    changed: 0,
                    deleted: 0,
                }),
            }
        );
    }

    #[test]
    fn report_counts_csv_rows_and_shows_csv_summaries() {
        let t = Theme::dark(false);
        let csv = vec![CsvPending {
            name: "tasks.csv".to_string(),
            push: CsvSideDiff {
                added: 2,
                changed: 1,
                deleted: 0,
            },
            pull: Some(CsvSideDiff {
                added: 0,
                changed: 0,
                deleted: 1,
            }),
        }];

        let report = format_report(&[], &[], &csv, t);

        assert!(report.contains("Changes to push (3)"), "{report:?}");
        assert!(report.contains("tasks.csv: +2 ~1 -0 rows"), "{report:?}");
        assert!(report.contains("Changes to pull (1)"), "{report:?}");
        assert!(report.contains("tasks.csv: +0 ~0 -1 rows"), "{report:?}");
        assert!(
            report.contains("Run `brain sync` to push and pull all changes."),
            "{report:?}"
        );
    }

    #[test]
    fn report_explains_csv_deltas_are_baseline_diffs_not_provenance() {
        let t = Theme::dark(false);
        let csv = vec![CsvPending {
            name: "tasks.csv".to_string(),
            push: CsvSideDiff {
                added: 1,
                ..Default::default()
            },
            pull: Some(CsvSideDiff {
                changed: 1,
                ..Default::default()
            }),
        }];

        let report = format_report(&[], &[], &csv, t);

        assert!(
            report.contains("CSV rows are compared against this machine's cached baseline"),
            "{report:?}"
        );
        assert!(
            report.contains("not proof that another machine made the change"),
            "{report:?}"
        );
        assert!(
            report.contains("brain sync will merge tasks.csv/habits.csv by id"),
            "{report:?}"
        );
    }

    #[test]
    fn report_warns_when_remote_csv_was_not_checked() {
        let t = Theme::dark(false);
        let csv = vec![CsvPending {
            name: "tasks.csv".to_string(),
            push: CsvSideDiff::default(),
            pull: None,
        }];

        let report = format_report(&[], &[], &csv, t);

        assert!(
            report.contains("Could not check remote CSV changes for tasks.csv."),
            "{report:?}"
        );
        assert!(!report.contains("In sync"), "{report:?}");
        assert!(!report.contains("Run `brain sync`"), "{report:?}");
    }

    #[test]
    fn collect_csv_pending_reads_baseline_local_and_remote_without_writes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let local_path = dir.path().join("tasks/tasks.csv");
        std::fs::create_dir_all(local_path.parent().expect("parent")).expect("mkdir");
        let base = "task_id,title,status\n1,base,open\n";
        let local = "task_id,title,status\n1,local,open\n2,local add,open\n";
        let remote = "task_id,title,status\n";
        std::fs::write(&local_path, local).expect("write local");

        let pending = collect_csv_pending_with_fetch(
            dir.path(),
            &["tasks/tasks.csv"],
            |name| {
                assert_eq!(name, "tasks.csv");
                Ok(base.to_string())
            },
            |rel| {
                assert_eq!(rel, "tasks/tasks.csv");
                Some(remote.to_string())
            },
        )
        .unwrap();

        assert_eq!(
            pending,
            vec![CsvPending {
                name: "tasks.csv".to_string(),
                push: CsvSideDiff {
                    added: 1,
                    changed: 1,
                    deleted: 0
                },
                pull: Some(CsvSideDiff {
                    added: 0,
                    changed: 0,
                    deleted: 1
                }),
            }]
        );
        assert_eq!(
            std::fs::read_to_string(local_path).expect("read local"),
            local
        );
    }

    #[test]
    fn active_schema_v2_diff_keys_distinct_uuids_with_one_display_id() {
        let dir = tempfile::tempdir().unwrap();
        let tasks = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::write(
            tasks.join("SCHEMA.json"),
            r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#,
        )
        .unwrap();
        let base = "task_uuid,task_id,assigned_to,system_key\n\
                    10000000-0000-4000-8000-000000000001,T1,member-a,\n";
        let local = "task_uuid,task_id,assigned_to,system_key\n\
                     20000000-0000-4000-8000-000000000001,T1,member-a,\n";
        std::fs::write(tasks.join("tasks.csv"), local).unwrap();

        let pending = collect_csv_pending_with_fetch(
            dir.path(),
            &["tasks/tasks.csv"],
            |_| Ok(base.to_owned()),
            |_| Some(base.to_owned()),
        )
        .unwrap();

        assert_eq!(pending[0].push.added, 1);
        assert_eq!(pending[0].push.deleted, 1);
        assert_eq!(pending[0].push.changed, 0);
    }

    #[test]
    fn inactive_schema_hybrid_diff_remains_task_id_keyed() {
        let dir = tempfile::tempdir().unwrap();
        let tasks = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let base = "task_id,task_uuid,status\nT1,,not_started\n";
        let local = "task_id,task_uuid,status\n\
                     T1,,done\n\
                     T2,20000000-0000-4000-8000-000000000002,not_started\n";
        std::fs::write(tasks.join("tasks.csv"), local).unwrap();

        let pending = collect_csv_pending_with_fetch(
            dir.path(),
            &["tasks/tasks.csv"],
            |_| Ok(base.to_owned()),
            |_| Some(base.to_owned()),
        )
        .unwrap();

        assert_eq!(pending[0].push.added, 1);
        assert_eq!(pending[0].push.changed, 1);
        assert_eq!(pending[0].push.deleted, 0);
    }

    #[test]
    fn invalid_check_generations_are_labeled_and_leave_every_store_unchanged() {
        use std::cell::RefCell;
        use std::collections::BTreeMap;

        for (generation, baseline, local, remote) in [
            (
                "baseline",
                "task_id,status\nT1,open\nT1,done\n",
                "task_id,status\nT1,open\n",
                "task_id,status\nT1,open\n",
            ),
            (
                "local",
                "task_id,status\nT1,open\n",
                "task_id,status\nT1,open,extra\n",
                "task_id,status\nT1,open\n",
            ),
            (
                "remote",
                "task_id,status\nT1,open\n",
                "task_id,status\nT1,open\n",
                "task_id,status\nT1,open\nT1,done\n",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let tasks = dir.path().join("tasks");
            let project = dir.path().join("projects/alpha");
            std::fs::create_dir_all(&tasks).unwrap();
            std::fs::create_dir_all(&project).unwrap();
            std::fs::write(tasks.join("tasks.csv"), local).unwrap();
            std::fs::write(tasks.join("habits.csv"), "task_id,status\nH1,open\n").unwrap();
            std::fs::write(tasks.join(".tasks_next_id"), "2\n").unwrap();
            std::fs::write(tasks.join(".habits_next_id"), "2\n").unwrap();
            std::fs::write(project.join(".METADATA.json"), b"{\"name\":\"alpha\"}\n").unwrap();
            let baseline_path = dir.path().join("cache/baselines/tasks.csv");
            std::fs::create_dir_all(baseline_path.parent().unwrap()).unwrap();
            std::fs::write(&baseline_path, baseline).unwrap();
            let remote_store = RefCell::new(BTreeMap::from([(
                "tasks/tasks.csv".to_owned(),
                remote.to_owned(),
            )]));
            let snapshots = [
                (
                    tasks.join("tasks.csv"),
                    std::fs::read(tasks.join("tasks.csv")).unwrap(),
                ),
                (
                    tasks.join("habits.csv"),
                    std::fs::read(tasks.join("habits.csv")).unwrap(),
                ),
                (
                    tasks.join(".tasks_next_id"),
                    std::fs::read(tasks.join(".tasks_next_id")).unwrap(),
                ),
                (
                    tasks.join(".habits_next_id"),
                    std::fs::read(tasks.join(".habits_next_id")).unwrap(),
                ),
                (
                    project.join(".METADATA.json"),
                    std::fs::read(project.join(".METADATA.json")).unwrap(),
                ),
                (
                    baseline_path.clone(),
                    std::fs::read(&baseline_path).unwrap(),
                ),
            ];
            let remote_before = remote_store.borrow().clone();

            let error = collect_csv_pending_with_fetch(
                dir.path(),
                &["tasks/tasks.csv"],
                |_| std::fs::read_to_string(&baseline_path).map_err(|error| error.to_string()),
                |relative| remote_store.borrow().get(relative).cloned(),
            )
            .unwrap_err();

            let message = error.to_string();
            assert!(message.contains(generation), "{message}");
            assert!(message.contains("tasks/tasks.csv"), "{message}");
            for (path, before) in snapshots {
                assert_eq!(std::fs::read(path).unwrap(), before);
            }
            assert_eq!(*remote_store.borrow(), remote_before);
        }
    }

    #[test]
    fn invalid_manifest_and_csv_render_warning_without_false_clean_claim() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        std::fs::write(dir.path().join("tasks/SCHEMA.json"), "not json\n").unwrap();
        std::fs::write(
            dir.path().join("tasks/tasks.csv"),
            "task_id,status\nT1,open\n",
        )
        .unwrap();

        let error = collect_csv_pending_with_fetch(
            dir.path(),
            &["tasks/tasks.csv"],
            |_| Ok("task_id,status\nT1,open\n".to_owned()),
            |_| Some("task_id,status\nT1,open\n".to_owned()),
        )
        .unwrap_err();
        let warning = format_csv_check_error(&error, Theme::dark(false));

        assert!(warning.contains("Could not check task and habit CSV changes"));
        assert!(warning.contains("tasks/SCHEMA.json"));
        assert!(!warning.contains("In sync"));
    }

    #[test]
    fn baseline_read_failure_is_labeled_instead_of_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        std::fs::write(
            dir.path().join("tasks/tasks.csv"),
            "task_id,status\nT1,open\n",
        )
        .unwrap();

        let error = collect_csv_pending_with_fetch(
            dir.path(),
            &["tasks/tasks.csv"],
            |_| Err("permission denied".to_owned()),
            |_| None,
        )
        .unwrap_err();

        assert!(error.to_string().contains("baseline tasks/tasks.csv"));
        assert!(error.to_string().contains("permission denied"));
    }
}
