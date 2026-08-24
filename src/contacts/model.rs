//! The contact record and the CSV it lives in.
//!
//! One CSV at `<brain-root>/resources/contacts/contacts.csv`, with a stable
//! `C###` per contact. Rows are always written back in id order, so the file
//! stays diffable and syncs cleanly.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::tasks::complete::{CsvFile, Row, field, read_csv, write_csv};

/// Every column, in file order.
pub(crate) const COLUMNS: [&str; 13] = [
    "id",
    "name",
    "job",
    "company",
    "email",
    "phone",
    "preferred_comms",
    "address",
    "tags",
    "birthday",
    "notes",
    "created_date",
    "last_updated",
];

/// Fields `find` searches when no single field is named.
pub(crate) const SEARCH_FIELDS: [&str; 8] = [
    "name", "job", "company", "email", "phone", "address", "tags", "notes",
];

/// Accepted `preferred_comms` values.
pub(crate) const PREFERRED_COMMS: [&str; 3] = ["email", "whatsapp", "phone"];

/// One contact, rendered as JSON in column order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Contact(pub(crate) serde_json::Map<String, serde_json::Value>);

impl Contact {
    pub(crate) fn from_row(row: &Row) -> Self {
        Self(
            COLUMNS
                .iter()
                .map(|column| {
                    (
                        (*column).to_owned(),
                        serde_json::Value::String(field(row, column)),
                    )
                })
                .collect(),
        )
    }

    pub(crate) fn get(&self, column: &str) -> &str {
        self.0
            .get(column)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    }
}

pub(crate) fn csv_path(root: &Path) -> PathBuf {
    root.join("resources/contacts/contacts.csv")
}

pub(crate) fn config_path(root: &Path) -> PathBuf {
    root.join("resources/contacts/contacts.config.json")
}

/// The numeric part of a `C###` id, or 0.
pub(crate) fn id_number(id: &str) -> u32 {
    id.strip_prefix(['C', 'c'])
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// The next free id, zero-padded to three digits.
pub(crate) fn next_id(rows: &[Row]) -> String {
    let highest = rows
        .iter()
        .map(|row| id_number(&field(row, "id")))
        .max()
        .unwrap_or(0);
    format!("C{:03}", highest + 1)
}

pub(crate) fn load(root: &Path) -> Result<Vec<Row>> {
    let path = csv_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(read_csv(&path)?.rows)
}

/// Write every row back in id order, with the full column set.
pub(crate) fn save(root: &Path, rows: &[Row]) -> Result<()> {
    let path = csv_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut ordered = rows.to_vec();
    ordered.sort_by_key(|row| id_number(&field(row, "id")));
    let csv = CsvFile {
        header: COLUMNS.iter().map(|column| (*column).to_owned()).collect(),
        rows: ordered,
    };
    write_csv(&path, &csv)
}
