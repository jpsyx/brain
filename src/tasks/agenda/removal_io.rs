//! Removal-only derived-output results, separate from ordinary completion.
use super::{RemovalAgendaChange, Targets};
use chrono::NaiveDate;

/// A precise downstream failure boundary; no private path or row values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemovalStage {
    AgendaRead,
    SnapshotRead,
    Transform,
    AgendaWrite,
    PdfRender,
}

/// Best-effort refresh after a durable canonical commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemovalOutcome {
    NoAgenda,
    Unchanged,
    Updated { pdf: bool },
    Warning { stage: RemovalStage },
}

impl RemovalStage {
    /// Stable safe receipt detail, independent of private paths and diagnostics.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AgendaRead => "agenda_read",
            Self::SnapshotRead => "snapshot_read",
            Self::Transform => "transform",
            Self::AgendaWrite => "agenda_write",
            Self::PdfRender => "pdf_render",
        }
    }
}

/// Refresh exactly one batch. An unacknowledged local commit retries an
/// existing printable even when a previous attempt already wrote the Markdown.
pub(crate) fn sync_removal_targets(
    targets: &Targets,
    changes: &[RemovalAgendaChange],
    today: NaiveDate,
) -> RemovalOutcome {
    let text = match std::fs::read_to_string(&targets.markdown) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RemovalOutcome::NoAgenda;
        }
        Err(_) => return warning(RemovalStage::AgendaRead),
    };
    let read = |name: &str| -> anyhow::Result<crate::tasks::complete::CsvFile> {
        crate::tasks::complete::parse_csv_bytes(&std::fs::read(targets.tasks_dir.join(name))?)
    };
    let (Ok(tasks), Ok(habits)) = (read("tasks.csv"), read("habits.csv")) else {
        return warning(RemovalStage::SnapshotRead);
    };
    let snapshot = super::Snapshot {
        tasks: &tasks.rows,
        habits: &habits.rows,
    };
    let Ok(synced) = super::sync_removal_markdown(&text, changes, &snapshot, today) else {
        return warning(RemovalStage::Transform);
    };
    let needs_write = synced != text;
    if needs_write && std::fs::write(&targets.markdown, &synced).is_err() {
        return warning(RemovalStage::AgendaWrite);
    }
    // NotFound is the only clean absence. Metadata failures must not conceal a
    // potentially stale printable as a successful refresh.
    match std::fs::symlink_metadata(&targets.pdf) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if needs_write {
                RemovalOutcome::Updated { pdf: false }
            } else {
                RemovalOutcome::Unchanged
            }
        }
        Ok(_) => match super::removal_pdf::replace_printable(targets, &synced) {
            Ok(()) => RemovalOutcome::Updated { pdf: true },
            Err(_) => warning(RemovalStage::PdfRender),
        },
        Err(_) => warning(RemovalStage::PdfRender),
    }
}

fn warning(stage: RemovalStage) -> RemovalOutcome {
    crate::logging::log(format!(
        "removal agenda refresh failed at {}",
        stage.as_str()
    ));
    RemovalOutcome::Warning { stage }
}
