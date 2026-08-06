//! Pure legacy CSV and schema-metadata conversion.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};

use super::TASK_SCHEMA_VERSION;
use crate::tasks::identity::{CsvKind, TaskUuid, legacy_task_uuid};
use crate::users::AssignmentRewrites;
use crate::workspace::WorkspaceId;

pub(super) fn is_current(tasks: &[u8], habits: &[u8], schema: &[u8]) -> Result<bool> {
    let schema: Value = serde_json::from_slice(schema).context("parsing tasks/SCHEMA.json")?;
    Ok(
        schema.get("task_schema_version").and_then(Value::as_u64) == Some(TASK_SCHEMA_VERSION)
            && schema.get("merge_key").and_then(Value::as_str) == Some("task_uuid")
            && schema
                .pointer("/display_identity/field")
                .and_then(Value::as_str)
                == Some("task_id")
            && schema
                .pointer("/display_identity/mutable")
                .and_then(Value::as_bool)
                == Some(true)
            && csv_has_current_identity(tasks)?
            && csv_has_current_identity(habits)?,
    )
}

pub(super) fn schema_version(schema: &[u8]) -> Result<Option<u64>> {
    let schema: Value = serde_json::from_slice(schema).context("parsing tasks/SCHEMA.json")?;
    Ok(schema.get("task_schema_version").and_then(Value::as_u64))
}

fn csv_has_current_identity(bytes: &[u8]) -> Result<bool> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let headers = reader.headers()?.clone();
    if headers.get(0) != Some("task_uuid")
        || !headers.iter().any(|column| column == "task_id")
        || !headers.iter().any(|column| column == "assigned_to")
        || !headers.iter().any(|column| column == "system_key")
        || headers.iter().any(|column| column == "assignee")
    {
        return Ok(false);
    }
    let Some(uuid_index) = headers.iter().position(|column| column == "task_uuid") else {
        return Ok(false);
    };
    for record in reader.records() {
        let record = record?;
        if TaskUuid::parse(record.get(uuid_index).unwrap_or_default()).is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn migrate_csv(
    bytes: &[u8],
    workspace_id: WorkspaceId,
    kind: CsvKind,
    rewrites: &AssignmentRewrites,
) -> Result<Vec<u8>> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let source_header = reader
        .headers()?
        .iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !source_header.iter().any(|column| column == "task_id") {
        bail!("{} CSV is missing task_id", kind.as_str());
    }
    reject_duplicate_columns(&source_header, kind)?;
    let header = migrated_header(&source_header);
    let has_assigned_to = source_header.iter().any(|column| column == "assigned_to");
    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let record =
            record.with_context(|| format!("parsing {} row {}", kind.as_str(), index + 2))?;
        let mut row = source_header
            .iter()
            .enumerate()
            .map(|(column_index, column)| {
                (
                    column.clone(),
                    record.get(column_index).unwrap_or_default().to_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let display_id = row.get("task_id").map_or("", String::as_str).trim();
        if display_id.is_empty() {
            bail!("{} row {} has an empty task_id", kind.as_str(), index + 2);
        }
        let task_uuid = match row.get("task_uuid").map(String::as_str).map(str::trim) {
            Some(existing) if !existing.is_empty() => {
                TaskUuid::parse(existing).with_context(|| {
                    format!("invalid task_uuid on {} row {}", kind.as_str(), index + 2)
                })?
            }
            _ => legacy_task_uuid(workspace_id, kind, display_id),
        };
        row.insert("task_uuid".to_owned(), task_uuid.to_string());
        if has_assigned_to {
            row.remove("assignee");
        } else {
            let assignment = row.remove("assignee").unwrap_or_default();
            row.insert("assigned_to".to_owned(), assignment);
        }
        if let Some(assignment) = row.get("assigned_to") {
            let rewritten = rewrites.apply(assignment).to_owned();
            row.insert("assigned_to".to_owned(), rewritten);
        }
        row.entry("system_key".to_owned()).or_default();
        rows.push(row);
    }

    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer.write_record(&header)?;
    for row in rows {
        writer.write_record(
            header
                .iter()
                .map(|column| row.get(column).map_or("", String::as_str)),
        )?;
    }
    writer
        .into_inner()
        .map_err(csv::IntoInnerError::into_error)
        .map_err(Into::into)
}

pub(super) fn repair_duplicate_uuids(
    tasks: &[u8],
    habits: &[u8],
    workspace_id: WorkspaceId,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut used = HashSet::new();
    let tasks = repair_duplicate_uuids_in_csv(tasks, workspace_id, CsvKind::Tasks, &mut used)?;
    let habits = repair_duplicate_uuids_in_csv(habits, workspace_id, CsvKind::Habits, &mut used)?;
    Ok((tasks, habits))
}

fn repair_duplicate_uuids_in_csv(
    bytes: &[u8],
    workspace_id: WorkspaceId,
    kind: CsvKind,
    used: &mut HashSet<String>,
) -> Result<Vec<u8>> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let headers = reader.headers()?.clone();
    let uuid_index = headers
        .iter()
        .position(|column| column == "task_uuid")
        .ok_or_else(|| anyhow!("{} CSV is missing task_uuid", kind.as_str()))?;
    let task_id_index = headers
        .iter()
        .position(|column| column == "task_id")
        .ok_or_else(|| anyhow!("{} CSV is missing task_id", kind.as_str()))?;
    let mut changed = false;
    let mut rows = Vec::new();
    for (row_index, record) in reader.records().enumerate() {
        let mut row = record
            .with_context(|| format!("parsing {} row {}", kind.as_str(), row_index + 2))?
            .iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let uuid = row.get(uuid_index).map_or("", String::as_str).trim();
        uuid::Uuid::parse_str(uuid).with_context(|| {
            format!(
                "invalid task_uuid on {} row {}",
                kind.as_str(),
                row_index + 2
            )
        })?;
        if !used.insert(uuid.to_owned()) {
            let display_id = row.get(task_id_index).map_or("", String::as_str).trim();
            let mut attempt = 0_u32;
            let replacement = loop {
                let seed = format!(
                    "{workspace_id}:{}:duplicate:{uuid}:{display_id}:{row_index}:{attempt}",
                    kind.as_str()
                );
                let candidate =
                    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, seed.as_bytes()).to_string();
                if used.insert(candidate.clone()) {
                    break candidate;
                }
                attempt = attempt.saturating_add(1);
            };
            row[uuid_index] = replacement;
            changed = true;
        }
        rows.push(row);
    }
    if !changed {
        return Ok(bytes.to_vec());
    }
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer.write_record(&headers)?;
    for row in rows {
        writer.write_record(row)?;
    }
    Ok(writer.into_inner()?)
}

fn reject_duplicate_columns(header: &[String], kind: CsvKind) -> Result<()> {
    let mut seen = HashSet::new();
    if let Some(column) = header.iter().find(|column| !seen.insert(column.as_str())) {
        bail!("{} CSV has duplicate column {column}", kind.as_str());
    }
    Ok(())
}

fn migrated_header(source: &[String]) -> Vec<String> {
    let has_assigned_to = source.iter().any(|column| column == "assigned_to");
    let mut header = vec!["task_uuid".to_owned(), "task_id".to_owned()];
    for column in source {
        match column.as_str() {
            "task_uuid" | "task_id" => {}
            "assignee" if has_assigned_to => {}
            "assignee" => header.push("assigned_to".to_owned()),
            _ => header.push(column.clone()),
        }
    }
    if !header.iter().any(|column| column == "assigned_to") {
        header.push("assigned_to".to_owned());
    }
    if !header.iter().any(|column| column == "system_key") {
        header.push("system_key".to_owned());
    }
    super::canonical_current_header(&header)
}

pub(super) fn migrate_schema_metadata(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(bytes).context("parsing tasks/SCHEMA.json")?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("tasks/SCHEMA.json must contain a JSON object"))?;
    object.insert(
        "task_schema_version".to_owned(),
        Value::from(TASK_SCHEMA_VERSION),
    );
    object.insert(
        "merge_key".to_owned(),
        Value::String("task_uuid".to_owned()),
    );
    object.insert(
        "display_identity".to_owned(),
        Value::Object(Map::from_iter([
            ("field".to_owned(), Value::String("task_id".to_owned())),
            ("mutable".to_owned(), Value::Bool(true)),
        ])),
    );
    object.insert("forward_compatible_columns".to_owned(), Value::Bool(true));
    object.insert(
        "identity".to_owned(),
        json!({
            "task_uuid": "immutable UUID merge identity",
            "task_id": "mutable human-facing display identity"
        }),
    );
    let mut output = serde_json::to_vec_pretty(&value)?;
    output.push(b'\n');
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{migrate_csv, migrate_schema_metadata, migrated_header, repair_duplicate_uuids};
    use crate::tasks::identity::CsvKind;
    use crate::users::{AssignmentRewrites, UserId};
    use crate::workspace::WorkspaceId;

    #[test]
    fn migration_moves_every_mapped_legacy_assignment_onto_its_portable_member() {
        let workspace_id = WorkspaceId::parse("00000000-0000-4000-8000-000000000001").unwrap();
        let mut rewrites = AssignmentRewrites::new();
        rewrites.record("me", &UserId::parse("pablo").unwrap());
        rewrites.record("Wife", &UserId::parse("sam").unwrap());

        let migrated = migrate_csv(
            b"task_id,task_name,assignee\nT1,Plan,me\nT2,Walk,Wife\nT3,Rest,pablo\nT4,Idle,\n",
            workspace_id,
            CsvKind::Tasks,
            &rewrites,
        )
        .unwrap();

        let mut reader = csv::Reader::from_reader(migrated.as_slice());
        let headers = reader.headers().unwrap().clone();
        assert!(!headers.iter().any(|header| header == "assignee"));
        let index = headers
            .iter()
            .position(|header| header == "assigned_to")
            .unwrap();
        let assignments = reader
            .records()
            .map(|row| row.unwrap().get(index).unwrap_or_default().to_owned())
            .collect::<Vec<_>>();

        assert_eq!(assignments, ["pablo", "sam", "pablo", ""]);
    }

    #[test]
    fn migration_canonicalizes_all_known_columns_and_sorts_forward_compatible_columns() {
        let first = [
            "notes",
            "z_extension",
            "task_id",
            "status",
            "a_extension",
            "assigned_to",
        ]
        .map(str::to_owned);
        let second = [
            "a_extension",
            "assigned_to",
            "status",
            "task_id",
            "z_extension",
            "notes",
        ]
        .map(str::to_owned);
        let expected = [
            "task_uuid",
            "task_id",
            "status",
            "assigned_to",
            "notes",
            "system_key",
            "a_extension",
            "z_extension",
        ]
        .map(str::to_owned);

        assert_eq!(migrated_header(&first), expected);
        assert_eq!(migrated_header(&second), expected);
    }

    #[test]
    fn migrated_schema_declares_forward_compatible_column_preservation() {
        let migrated = migrate_schema_metadata(b"{}").unwrap();
        let metadata: serde_json::Value = serde_json::from_slice(&migrated).unwrap();

        assert_eq!(metadata["forward_compatible_columns"], true);
    }

    #[test]
    fn duplicate_existing_uuids_are_reassigned_deterministically_across_csvs() {
        let duplicate = "10000000-0000-4000-8000-000000000001";
        let tasks = format!("task_uuid,task_id\n{duplicate},T1\n");
        let habits = format!("task_uuid,task_id\n{duplicate},H1\n");
        let workspace_id = WorkspaceId::parse("00000000-0000-4000-8000-000000000001").unwrap();

        let (repaired_tasks, repaired_habits) =
            repair_duplicate_uuids(tasks.as_bytes(), habits.as_bytes(), workspace_id).unwrap();

        assert_eq!(
            String::from_utf8(repaired_tasks.clone()).unwrap(),
            tasks,
            "the first occurrence keeps its existing identity"
        );
        assert_ne!(repaired_habits, habits.as_bytes());
        let repaired_habits_text = String::from_utf8(repaired_habits.clone()).unwrap();
        assert!(repaired_habits_text.contains("H1"));
        assert!(!repaired_habits_text.contains(duplicate));
        assert_eq!(
            (repaired_tasks, repaired_habits),
            repair_duplicate_uuids(tasks.as_bytes(), habits.as_bytes(), workspace_id).unwrap()
        );
    }
}
