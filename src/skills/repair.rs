//! Explicit registry repair owns startup, one refresh attempt and one report.
use super::reconcile::{self, RefreshMode, RefreshReport};
use crate::workspace::RegistryStore;
use anyhow::{Result, ensure};
use serde::Serialize;

/// Classify the explicit repair before inherited workspace selection or IO.
#[must_use]
pub fn is_requested(cli: &crate::cli::Cli) -> bool {
    matches!(&cli.command, Some(crate::cli::Cmd::Skills(args)) if matches!(args.action, Some(crate::cli::SkillsAction::Sync { all_workspaces: true, .. })))
}

/// One invocation's attempt, retaining fatal enumeration failures as well as rows.
/// The managed-skills migration callback may fill this slot before dispatch.
pub(crate) struct RefreshAttempt {
    mode: RefreshMode,
    state: AttemptState,
}
enum AttemptState {
    NotRun,
    Finished(RefreshReport),
}
impl RefreshAttempt {
    pub(crate) const fn new(mode: RefreshMode) -> Self {
        Self {
            mode,
            state: AttemptState::NotRun,
        }
    }
    /// Retain this request's explicit-source or compatibility-only semantics.
    pub(crate) fn run(
        &mut self,
        home: Option<&std::path::Path>,
        store: &RegistryStore,
    ) -> &RefreshReport {
        self.finish(|mode| {
            home.map_or_else(
                || RefreshReport {
                    workspaces: Vec::new(),
                    refresh_error: Some("HOME is not set".into()),
                },
                |home| reconcile::all(home, store, mode),
            )
        })
    }
    fn finish(&mut self, run: impl FnOnce(RefreshMode) -> RefreshReport) -> &RefreshReport {
        if matches!(self.state, AttemptState::NotRun) {
            self.state = AttemptState::Finished(run(self.mode));
        }
        let AttemptState::Finished(report) = &self.state else {
            unreachable!("attempt was stored")
        };
        report
    }
}

#[derive(Serialize)]
struct RepairReport {
    schema_version: u32,
    #[serde(flatten)]
    refresh: RefreshReport,
    startup_error: Option<String>,
    logger_error: Option<String>,
}
impl RepairReport {
    fn failed(&self) -> bool {
        self.startup_error.is_some() || self.logger_error.is_some() || self.refresh.failed()
    }
}

/// Attempt mandatory startup once and complete the independent requested repair.
/// Returns a rendered exit code; the caller must not render a second error body.
pub fn run(cli: &crate::cli::Cli) -> Result<i32> {
    ensure!(is_requested(cli), "expected all-workspace skills repair");
    ensure!(
        cli.workspace_selector.is_none(),
        "--workspace conflicts with --all-workspaces"
    );
    let mut attempt = RefreshAttempt::new(RefreshMode::Explicit);
    // The dedicated migration callback can populate this same slot when added.
    // Ordinary startup and installer migration retain their fail-fast behavior.
    let startup = crate::startup_migration::run_current();
    let logger = crate::logging::init(cli.verbose, false);
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let (report, guard) = finish_report(
        &mut attempt,
        home.as_deref(),
        &RegistryStore::real(),
        startup,
        logger,
    );
    let json = matches!(&cli.command, Some(crate::cli::Cmd::Skills(args)) if matches!(args.action, Some(crate::cli::SkillsAction::Sync { json: true, .. })));
    if json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        for row in &report.refresh.workspaces {
            eprintln!("{}: {:?}", row.name, row.outcome);
        }
        for message in [
            &report.startup_error,
            &report.logger_error,
            &report.refresh.refresh_error,
        ]
        .into_iter()
        .flatten()
        {
            eprintln!("{message}");
        }
    }
    let code = i32::from(report.failed());
    drop(guard);
    Ok(code)
}

fn finish_report(
    attempt: &mut RefreshAttempt,
    home: Option<&std::path::Path>,
    store: &RegistryStore,
    startup: Result<()>,
    logger: std::io::Result<crate::logging::Guard>,
) -> (RepairReport, Option<crate::logging::Guard>) {
    let startup_error = startup.err().map(|_| "mandatory startup migrations failed; inspect workspace hooks and machine migration state".into());
    let (guard, logger_error) = match logger {
        Ok(guard) => (Some(guard), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let refresh = attempt.run(home, store).clone();
    (
        RepairReport {
            schema_version: 1,
            refresh,
            startup_error,
            logger_error,
        },
        guard,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_attempt_is_retained_without_running_again() {
        let mut attempt = RefreshAttempt::new(RefreshMode::Explicit);
        let failed = RefreshReport {
            workspaces: Vec::new(),
            refresh_error: Some("registry unavailable".into()),
        };
        assert!(
            attempt
                .finish(|mode| {
                    assert!(matches!(mode, RefreshMode::Explicit));
                    failed
                })
                .failed()
        );
        assert!(
            attempt
                .finish(|_| panic!("must not retry a fatal attempt"))
                .failed()
        );
    }
    #[test]
    fn logger_failure_retains_independent_repair_outcomes() {
        use crate::workspace::{WorkspaceId, WorkspaceManifest};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let id = WorkspaceId::new();
        WorkspaceManifest::new(id).write_new(&root).unwrap();
        let store = RegistryStore::from_path(temp.path().join("env.json"));
        std::fs::write(
            store.path(),
            serde_json::to_vec(&serde_json::json!({
                "schema_version":4, "default_workspace":"alpha", "workspaces": {
                    "alpha": {"workspace_id":id,"root":root,"local_user_id":"test-user"}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let mut attempt = RefreshAttempt::new(RefreshMode::Explicit);
        let (report, guard) = finish_report(
            &mut attempt,
            Some(temp.path()),
            &store,
            Ok(()),
            Err(std::io::Error::other("logger unavailable")),
        );
        assert!(guard.is_none());
        assert!(report.failed());
        assert_eq!(report.logger_error.as_deref(), Some("logger unavailable"));
        assert!(matches!(
            report.refresh.workspaces[0].outcome,
            reconcile::WorkspaceRefresh::Updated { .. }
        ));
        assert!(root.join(".agents/skills/todo/SKILL.md").is_file());
    }
}
