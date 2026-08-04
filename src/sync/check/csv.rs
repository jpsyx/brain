use std::collections::BTreeSet;
use std::path::Path;
use std::{error::Error, fmt};

use crate::sync::csv_merge::{SchemaStatus, Table, TableParseError, parse, schema_status};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CsvSideDiff {
    pub added: usize,
    pub changed: usize,
    pub deleted: usize,
}

impl CsvSideDiff {
    #[must_use]
    pub(super) fn total(self) -> usize {
        self.added + self.changed + self.deleted
    }

    #[must_use]
    pub(super) fn is_empty(self) -> bool {
        self.total() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvPending {
    pub name: String,
    pub push: CsvSideDiff,
    pub pull: Option<CsvSideDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvCheckError {
    Schema(String),
    Generation {
        generation: &'static str,
        relative: String,
        message: String,
    },
}

impl fmt::Display for CsvCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(message) => write!(formatter, "tasks/SCHEMA.json: {message}"),
            Self::Generation {
                generation,
                relative,
                message,
            } => write!(formatter, "{generation} {relative}: {message}"),
        }
    }
}

impl Error for CsvCheckError {}

pub fn diff_csv_rows(
    base: &str,
    side: &str,
    schema_status: SchemaStatus,
) -> Result<CsvSideDiff, TableParseError> {
    let base = parse(base, schema_status)?;
    let side = parse(side, schema_status)?;
    Ok(diff_tables(&base, &side))
}

fn diff_tables(base: &Table, side: &Table) -> CsvSideDiff {
    let header = base
        .header
        .iter()
        .chain(side.header.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut diff = CsvSideDiff::default();
    let ids: BTreeSet<&str> = base
        .rows
        .keys()
        .chain(side.rows.keys())
        .map(String::as_str)
        .collect();

    for id in ids {
        match (base.rows.get(id), side.rows.get(id)) {
            (None, Some(_)) => diff.added += 1,
            (Some(_), None) => diff.deleted += 1,
            (Some(base_row), Some(side_row))
                if base.aligned_row(base_row, &header) != side.aligned_row(side_row, &header) =>
            {
                diff.changed += 1;
            }
            _ => {}
        }
    }
    diff
}

pub fn csv_pending_from_texts(
    relative: &str,
    base: &str,
    local: &str,
    remote: Option<&str>,
    schema_status: SchemaStatus,
) -> Result<CsvPending, CsvCheckError> {
    let name = Path::new(relative)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(relative);
    let parse_generation = |generation: &'static str, text: &str| {
        parse(text, schema_status).map_err(|error| CsvCheckError::Generation {
            generation,
            relative: relative.to_owned(),
            message: error.to_string(),
        })
    };
    let base_table = parse_generation("baseline", base)?;
    let local_table = parse_generation("local", local)?;
    let remote_table = remote
        .map(|text| parse_generation("remote", text))
        .transpose()?;
    if base.trim().is_empty()
        && let Some(remote_table) = remote_table.as_ref()
    {
        if local_table == *remote_table {
            return Ok(CsvPending {
                name: name.to_owned(),
                push: CsvSideDiff::default(),
                pull: Some(CsvSideDiff::default()),
            });
        }
        if !local_table.rows.is_empty() && !remote_table.rows.is_empty() {
            return Ok(CsvPending {
                name: name.to_owned(),
                push: diff_tables(remote_table, &local_table),
                pull: Some(CsvSideDiff::default()),
            });
        }
    }
    Ok(CsvPending {
        name: name.to_owned(),
        push: diff_tables(&base_table, &local_table),
        pull: remote_table
            .as_ref()
            .map(|remote| diff_tables(&base_table, remote)),
    })
}

pub fn collect_csv_pending_with_fetch(
    root: &Path,
    csvs: &[&str],
    mut read_baseline: impl FnMut(&str) -> Result<String, String>,
    mut fetch_remote: impl FnMut(&str) -> Option<String>,
) -> Result<Vec<CsvPending>, CsvCheckError> {
    let manifest_path = root.join("tasks/SCHEMA.json");
    let manifest = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(CsvCheckError::Schema(format!("reading metadata: {error}"))),
    };
    let schema_status = schema_status(manifest.as_deref())
        .map_err(|error| CsvCheckError::Schema(format!("{error:#}")))?;
    csvs.iter()
        .map(|relative| {
            let name = Path::new(relative)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(relative);
            let base = read_baseline(name).map_err(|message| CsvCheckError::Generation {
                generation: "baseline",
                relative: (*relative).to_owned(),
                message,
            })?;
            let local = match std::fs::read_to_string(root.join(relative)) {
                Ok(text) => text,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(error) => {
                    return Err(CsvCheckError::Generation {
                        generation: "local",
                        relative: (*relative).to_owned(),
                        message: format!("reading CSV: {error}"),
                    });
                }
            };
            let remote = fetch_remote(relative);
            csv_pending_from_texts(relative, &base, &local, remote.as_deref(), schema_status)
        })
        .collect()
}

#[must_use]
pub fn format_csv_check_error(error: &CsvCheckError, theme: Theme) -> String {
    theme.warning(&format!(
        "Could not check task and habit CSV changes: {error}"
    ))
}
