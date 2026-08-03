//! Name-aligned CSV table parsing and deterministic serialization.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

/// A parsed task CSV keyed by its active merge identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Stable output column order.
    pub header: Vec<String>,
    /// Merge identity to row cells aligned with `header`.
    pub rows: BTreeMap<String, Vec<String>>,
}

impl Table {
    #[must_use]
    pub(crate) fn column(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|column| column == name)
    }

    #[must_use]
    pub(crate) fn merge_key(&self) -> Option<&str> {
        if self.column("task_uuid").is_some() {
            Some("task_uuid")
        } else if self.column("task_id").is_some() {
            Some("task_id")
        } else {
            None
        }
    }

    #[must_use]
    pub(crate) fn is_uuid_keyed(&self) -> bool {
        self.merge_key() == Some("task_uuid")
    }

    #[must_use]
    pub(crate) fn aligned_row(&self, row: &[String], target: &[String]) -> Vec<String> {
        target
            .iter()
            .map(|column| {
                self.column(column)
                    .and_then(|index| row.get(index))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }
}

/// Parse CSV text, using `task_uuid` when present and legacy `task_id`
/// otherwise. Empty text remains a valid empty legacy table.
#[must_use]
pub fn parse(text: &str) -> Table {
    if text.is_empty() {
        return Table {
            header: Vec::new(),
            rows: BTreeMap::new(),
        };
    }
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());
    let header = reader
        .headers()
        .map(|record| record.iter().map(str::to_owned).collect::<Vec<_>>())
        .unwrap_or_default();
    let width = header.len();
    let key_index = header
        .iter()
        .position(|column| column == "task_uuid")
        .or_else(|| header.iter().position(|column| column == "task_id"));
    let mut rows = BTreeMap::new();
    for (row_index, record) in reader.records().flatten().enumerate() {
        let mut cells = record.iter().map(str::to_owned).collect::<Vec<_>>();
        if cells.is_empty() {
            continue;
        }
        cells.resize(width, String::new());
        let key = key_index
            .and_then(|index| cells.get(index).cloned())
            .unwrap_or_else(|| format!("invalid-row-{row_index}"));
        rows.insert(key, cells);
    }
    Table { header, rows }
}

/// Serialize rows in deterministic merge-key order.
#[must_use]
pub fn serialize(table: &Table) -> String {
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    if !table.header.is_empty() {
        let _ = writer.write_record(&table.header);
    }
    for row in table.rows.values() {
        let _ = writer.write_record(row);
    }
    let bytes = writer.into_inner().unwrap_or_default();
    String::from_utf8(bytes).unwrap_or_default()
}

/// Validate tables against the portable task-schema manifest before sync
/// writes. Legacy tables remain accepted only while schema v2 is absent.
pub fn validate_for_merge(manifest: Option<&str>, tables: &[&Table]) -> Result<()> {
    for table in tables.iter().filter(|table| !table.header.is_empty()) {
        if !table.is_uuid_keyed() && table.column("task_id").is_none() {
            bail!("legacy task CSV is missing required task_id merge key");
        }
    }
    let uuid_tables = tables
        .iter()
        .filter(|table| !table.header.is_empty() && table.is_uuid_keyed())
        .count();
    let Some(manifest) = manifest else {
        if uuid_tables > 0 {
            bail!("task_uuid CSV requires tasks/SCHEMA.json task schema version 2");
        }
        return Ok(());
    };
    let metadata: serde_json::Value =
        serde_json::from_str(manifest).context("parsing tasks/SCHEMA.json")?;
    let Some(version) = metadata
        .get("task_schema_version")
        .and_then(serde_json::Value::as_u64)
    else {
        if uuid_tables > 0 {
            bail!("task_uuid CSV requires tasks/SCHEMA.json task schema version 2");
        }
        return Ok(());
    };
    if version != crate::tasks::schema::TASK_SCHEMA_VERSION {
        bail!(
            "task schema version {version} is unsupported; this Brain supports {}",
            crate::tasks::schema::TASK_SCHEMA_VERSION
        );
    }
    if metadata
        .get("merge_key")
        .and_then(serde_json::Value::as_str)
        != Some("task_uuid")
    {
        bail!("task schema version 2 must declare task_uuid as its merge key");
    }
    let preserve_unknown = metadata
        .get("forward_compatible_columns")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    for table in tables.iter().filter(|table| !table.header.is_empty()) {
        validate_current_table(table, preserve_unknown)?;
    }
    Ok(())
}

fn validate_current_table(table: &Table, preserve_unknown: bool) -> Result<()> {
    const REQUIRED: [&str; 4] = ["task_uuid", "task_id", "assigned_to", "system_key"];
    const KNOWN: [&str; 29] = [
        "task_uuid",
        "task_id",
        "task_name",
        "task_type",
        "status",
        "waiting_since",
        "priority",
        "due_date",
        "hard_deadline",
        "start_date",
        "assigned_to",
        "see_also",
        "notes",
        "project",
        "energy_level",
        "context",
        "estimated_duration",
        "blocked_by",
        "defer_count",
        "recur_interval",
        "recur_unit",
        "ideal_time",
        "created_date",
        "completed_date",
        "last_touched",
        "linear_issue",
        "system_key",
        "calendar_id",
        "waiting_for",
    ];
    for column in REQUIRED {
        if table.column(column).is_none() {
            bail!("task schema version 2 CSV is missing required column {column}");
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for column in &table.header {
        if !seen.insert(column) {
            bail!("task schema version 2 CSV has duplicate column {column}");
        }
        if !preserve_unknown && !KNOWN.contains(&column.as_str()) {
            bail!(
                "task schema version 2 CSV has unknown column {column}; SCHEMA.json must declare forward_compatible_columns=true to preserve it"
            );
        }
    }
    let uuid_index = table
        .column("task_uuid")
        .expect("required task_uuid was checked above");
    for row in table.rows.values() {
        let uuid = row.get(uuid_index).map_or("", String::as_str);
        uuid::Uuid::parse_str(uuid)
            .with_context(|| format!("task schema version 2 CSV has invalid task_uuid {uuid}"))?;
    }
    Ok(())
}
