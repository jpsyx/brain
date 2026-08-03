//! Pure display-ID relationship resolution and emission.

use std::collections::{BTreeMap, BTreeSet};

use super::Table;

const UUID_MARKER: &str = "uuid:";

pub(crate) fn resolve_side_references(table: &Table) -> Table {
    if !table.is_uuid_keyed() {
        return table.clone();
    }
    let Some(display_index) = table.column("task_id") else {
        return table.clone();
    };
    let display_to_uuid = table
        .rows
        .iter()
        .filter_map(|(uuid, row)| {
            let display = row.get(display_index)?.trim();
            (!display.is_empty()).then(|| (display.to_owned(), uuid.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    rewrite_column(table, "blocked_by", |token| {
        display_to_uuid
            .get(token)
            .map(|uuid| format!("{UUID_MARKER}{uuid}"))
    })
}

pub(crate) fn emit_final_references(table: &Table) -> Table {
    if !table.is_uuid_keyed() {
        return table.clone();
    }
    let Some(display_index) = table.column("task_id") else {
        return table.clone();
    };
    let uuid_to_display = table
        .rows
        .iter()
        .filter_map(|(uuid, row)| Some((uuid.clone(), row.get(display_index)?.clone())))
        .collect::<BTreeMap<_, _>>();
    rewrite_column(table, "blocked_by", |token| {
        token
            .strip_prefix(UUID_MARKER)
            .and_then(|uuid| uuid_to_display.get(uuid).cloned())
    })
}

fn rewrite_column(
    table: &Table,
    column: &str,
    replacement: impl Fn(&str) -> Option<String>,
) -> Table {
    let Some(index) = table.column(column) else {
        return table.clone();
    };
    let mut rewritten = table.clone();
    for row in rewritten.rows.values_mut() {
        let Some(value) = row.get_mut(index) else {
            continue;
        };
        *value = rewrite_list(value, &replacement);
    }
    rewritten
}

fn rewrite_list(value: &str, replacement: &impl Fn(&str) -> Option<String>) -> String {
    let mut output = String::with_capacity(value.len());
    let mut token_start = 0;
    for (index, character) in value.char_indices() {
        if matches!(character, '|' | ',') {
            output.push_str(&rewrite_token(&value[token_start..index], replacement));
            output.push(character);
            token_start = index + character.len_utf8();
        }
    }
    output.push_str(&rewrite_token(&value[token_start..], replacement));
    output
}

fn rewrite_token(token: &str, replacement: &impl Fn(&str) -> Option<String>) -> String {
    let trimmed = token.trim();
    let Some(rewritten) = replacement(trimmed) else {
        return token.to_owned();
    };
    let leading = &token[..token.len() - token.trim_start().len()];
    let trailing = &token[token.trim_end().len()..];
    format!("{leading}{rewritten}{trailing}")
}

/// Derive canonical project reverse links from reconciled task tables.
#[must_use]
pub fn project_task_lists<'a>(
    tables: impl IntoIterator<Item = &'a Table>,
) -> BTreeMap<String, Vec<String>> {
    let mut projects = BTreeMap::<String, BTreeSet<String>>::new();
    for table in tables {
        let (Some(project_index), Some(display_index)) =
            (table.column("project"), table.column("task_id"))
        else {
            continue;
        };
        for row in table.rows.values() {
            let project = row.get(project_index).map_or("", String::as_str).trim();
            let display = row.get(display_index).map_or("", String::as_str).trim();
            if !project.is_empty() && !display.is_empty() {
                projects
                    .entry(project.to_owned())
                    .or_default()
                    .insert(display.to_owned());
            }
        }
    }
    projects
        .into_iter()
        .map(|(project, ids)| (project, ids.into_iter().collect()))
        .collect()
}

/// Replace only the project metadata `tasks` array, preserving every sibling.
pub fn rewrite_project_metadata(bytes: &[u8], task_ids: &[String]) -> serde_json::Result<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("tasks".to_owned(), serde_json::json!(task_ids));
    }
    let mut output = serde_json::to_vec_pretty(&value)?;
    output.push(b'\n');
    Ok(output)
}
