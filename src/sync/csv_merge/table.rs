//! Name-aligned CSV table parsing and deterministic serialization.

use std::collections::BTreeMap;
use std::{error::Error, fmt};

use anyhow::{Context, Result, bail};

/// A parsed task CSV keyed by its active merge identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Stable output column order.
    pub header: Vec<String>,
    /// Merge identity to row cells aligned with `header`.
    pub rows: BTreeMap<String, Vec<String>>,
    pub(crate) schema_status: SchemaStatus,
}

/// Task CSV identity activated by the portable schema metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaStatus {
    /// Coordinated migration is inactive; `task_id` remains merge identity.
    Legacy,
    /// Schema v2 is active; `task_uuid` is merge identity.
    Current,
}

/// Lossless parse failure detected before a task CSV may participate in sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableParseError {
    message: String,
}

impl TableParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TableParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for TableParseError {}

impl Table {
    #[must_use]
    pub(crate) fn column(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|column| column == name)
    }

    #[must_use]
    pub(crate) fn merge_key(&self) -> Option<&str> {
        match self.schema_status {
            SchemaStatus::Current => self.column("task_uuid").map(|_| "task_uuid"),
            SchemaStatus::Legacy => self.column("task_id").map(|_| "task_id"),
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

/// Parse CSV text without discarding malformed or duplicate records.
///
/// The caller supplies the identity status derived from `tasks/SCHEMA.json`.
/// This keeps compatibility files keyed by `task_id`, including new files
/// whose writers already populate `task_uuid`, until coordinated migration.
pub fn parse(
    text: &str,
    schema_status: SchemaStatus,
) -> std::result::Result<Table, TableParseError> {
    if text.is_empty() {
        return Ok(Table {
            header: Vec::new(),
            rows: BTreeMap::new(),
            schema_status,
        });
    }
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());
    let header = reader
        .headers()
        .map_err(|error| TableParseError::new(format!("malformed CSV header: {error}")))?
        .iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let width = header.len();
    let mut records = Vec::new();
    for (record_index, record) in reader.records().enumerate() {
        let row_number = record_index + 2;
        let record = record.map_err(|error| {
            TableParseError::new(format!("malformed CSV record at row {row_number}: {error}"))
        })?;
        if record.len() > width {
            return Err(TableParseError::new(format!(
                "malformed CSV record at row {row_number}: expected at most {width} fields, found {}",
                record.len()
            )));
        }
        let mut cells = record.iter().map(str::to_owned).collect::<Vec<_>>();
        if cells.is_empty() {
            continue;
        }
        cells.resize(width, String::new());
        records.push((row_number, cells));
    }
    let key_column = match schema_status {
        SchemaStatus::Current => Some("task_uuid"),
        SchemaStatus::Legacy => Some("task_id"),
    };
    let key_index = key_column.and_then(|column| header.iter().position(|name| name == column));
    let mut rows = BTreeMap::new();
    let mut first_rows = BTreeMap::new();
    for (row_number, cells) in records {
        let key = key_index
            .and_then(|index| cells.get(index).cloned())
            .unwrap_or_else(|| format!("invalid-row-{row_number}"));
        if let Some(column) = key_column {
            if key.trim().is_empty() {
                return Err(TableParseError::new(format!(
                    "missing {column} merge identity at row {row_number}"
                )));
            }
            if let Some(first_row) = first_rows.insert(key.clone(), row_number) {
                return Err(TableParseError::new(format!(
                    "duplicate {column} merge identity {key} at row {row_number} (first seen at row {first_row})"
                )));
            }
        }
        rows.insert(key, cells);
    }
    Ok(Table {
        header,
        rows,
        schema_status,
    })
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
    let status = schema_status(manifest)?;
    if tables.iter().any(|table| table.schema_status != status) {
        bail!("task CSV was parsed with identity inconsistent with tasks/SCHEMA.json");
    }
    if status == SchemaStatus::Legacy {
        for table in tables.iter().filter(|table| !table.header.is_empty()) {
            if table.column("task_id").is_none() {
                bail!("legacy task CSV is missing required task_id merge key");
            }
        }
        return Ok(());
    }
    let manifest = manifest.expect("current schema status requires metadata");
    let metadata: serde_json::Value = serde_json::from_str(manifest)
        .expect("schema_status already parsed and validated task metadata");
    let preserve_unknown = metadata
        .get("forward_compatible_columns")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    for table in tables.iter().filter(|table| !table.header.is_empty()) {
        validate_current_table(table, preserve_unknown)?;
    }
    Ok(())
}

/// Parse and validate the active identity declared by task schema metadata.
pub fn schema_status(manifest: Option<&str>) -> Result<SchemaStatus> {
    let Some(manifest) = manifest else {
        return Ok(SchemaStatus::Legacy);
    };
    let metadata: serde_json::Value =
        serde_json::from_str(manifest).context("parsing tasks/SCHEMA.json")?;
    let Some(version) = metadata
        .get("task_schema_version")
        .and_then(serde_json::Value::as_u64)
    else {
        return Ok(SchemaStatus::Legacy);
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
    Ok(SchemaStatus::Current)
}

/// Parse remote schema metadata, accepting the known pre-v2 task schema as legacy.
pub fn remote_schema_status(manifest: Option<&str>) -> Result<SchemaStatus> {
    let Some(manifest) = manifest else {
        return Ok(SchemaStatus::Legacy);
    };
    let metadata: serde_json::Value =
        serde_json::from_str(manifest).context("parsing remote tasks/SCHEMA.json")?;
    if is_known_legacy_manifest(&metadata) {
        return Ok(SchemaStatus::Legacy);
    }
    let version = metadata
        .get("task_schema_version")
        .ok_or_else(|| anyhow::anyhow!("remote task_schema_version is missing"))?
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("remote task_schema_version must be an unsigned integer"))?;
    if version != crate::tasks::schema::TASK_SCHEMA_VERSION {
        bail!(
            "remote task schema version {version} is unsupported; this Brain supports {}",
            crate::tasks::schema::TASK_SCHEMA_VERSION
        );
    }
    if metadata
        .get("merge_key")
        .and_then(serde_json::Value::as_str)
        != Some("task_uuid")
    {
        bail!("remote task schema version 2 must declare task_uuid as its merge key");
    }
    if metadata
        .pointer("/display_identity/field")
        .and_then(serde_json::Value::as_str)
        != Some("task_id")
    {
        bail!("remote task schema version 2 must declare task_id as its display identity");
    }
    if metadata
        .pointer("/display_identity/mutable")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        bail!("remote task schema version 2 must declare its display identity mutable");
    }
    Ok(SchemaStatus::Current)
}

fn is_known_legacy_manifest(metadata: &serde_json::Value) -> bool {
    ["tasks_csv", "habits_csv"].into_iter().all(|section| {
        let Some(section) = metadata.get(section).and_then(serde_json::Value::as_object) else {
            return false;
        };
        section.get("key").and_then(serde_json::Value::as_str) == Some("task_id")
            && section
                .get("columns")
                .is_some_and(serde_json::Value::is_array)
    })
}

fn validate_current_table(table: &Table, preserve_unknown: bool) -> Result<()> {
    const REQUIRED: [&str; 4] = ["task_uuid", "task_id", "assigned_to", "system_key"];
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
        if !preserve_unknown && !crate::tasks::schema::is_known_current_column(column) {
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

#[cfg(test)]
mod tests {
    use super::{SchemaStatus, parse, remote_schema_status};

    #[test]
    fn present_remote_schema_requires_a_typed_complete_supported_manifest() {
        let invalid = [
            "{}",
            r#"{"merge_key":"task_uuid"}"#,
            r#"{"task_schema_version":"3","merge_key":"task_uuid"}"#,
            r#"{"task_schema_version":2}"#,
            r#"{"task_schema_version":2,"merge_key":3}"#,
            r#"{"task_schema_version":3,"merge_key":"task_uuid"}"#,
            r#"{"task_schema_version":2,"merge_key":"task_uuid"}"#,
            r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id"}}"#,
            r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":"true"}}"#,
        ];

        assert_eq!(remote_schema_status(None).unwrap(), SchemaStatus::Legacy);
        for manifest in invalid {
            assert!(
                remote_schema_status(Some(manifest)).is_err(),
                "present remote schema was accepted: {manifest}"
            );
        }
        assert_eq!(
            remote_schema_status(Some(
                r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#
            ))
            .unwrap(),
            SchemaStatus::Current
        );
    }

    #[test]
    fn known_legacy_remote_schema_is_legacy() {
        let legacy = r#"{
            "tasks_csv": {"key": "task_id", "columns": []},
            "habits_csv": {"key": "task_id", "columns": []}
        }"#;

        assert_eq!(
            remote_schema_status(Some(legacy)).unwrap(),
            SchemaStatus::Legacy
        );
    }

    #[test]
    fn hybrid_legacy_rows_remain_keyed_by_task_id() {
        let table = parse(
            "task_id,task_uuid,status\n\
             T1,,not_started\n\
             T2,10000000-0000-4000-8000-000000000002,not_started\n",
            SchemaStatus::Legacy,
        )
        .unwrap();

        assert_eq!(table.merge_key(), Some("task_id"));
        assert_eq!(table.rows.len(), 2);
        assert!(table.rows.contains_key("T1"));
        assert!(table.rows.contains_key("T2"));
    }

    #[test]
    fn duplicate_current_task_uuid_is_rejected() {
        let error = parse(
            "task_uuid,task_id\n\
             10000000-0000-4000-8000-000000000001,T1\n\
             10000000-0000-4000-8000-000000000001,T2\n",
            SchemaStatus::Current,
        )
        .unwrap_err();

        assert!(error.to_string().contains("duplicate task_uuid"));
        assert!(error.to_string().contains("row 3"));
    }

    #[test]
    fn duplicate_legacy_task_id_is_rejected() {
        let error = parse(
            "task_id,status\nT1,not_started\nT1,done\n",
            SchemaStatus::Legacy,
        )
        .unwrap_err();

        assert!(error.to_string().contains("duplicate task_id"));
        assert!(error.to_string().contains("row 3"));
    }

    #[test]
    fn malformed_csv_record_is_rejected_with_its_row() {
        let error = parse(
            "task_id,notes\nT1,ok\nT2,ok,unexpected\n",
            SchemaStatus::Legacy,
        )
        .unwrap_err();

        assert!(error.to_string().contains("malformed CSV record"));
        assert!(error.to_string().contains("row 3"));
    }
}
