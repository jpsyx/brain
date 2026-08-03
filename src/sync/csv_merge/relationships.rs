//! Pure display-ID relationship resolution and emission.

use std::collections::{BTreeMap, BTreeSet};

use super::Table;

const UUID_MARKER: &str = "uuid:";
const DISPLAY_FALLBACK_MARKER: &str = ";display:";

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
    let replacement = |token: &str| {
        display_to_uuid
            .get(token)
            .map(|uuid| format!("{UUID_MARKER}{uuid}{DISPLAY_FALLBACK_MARKER}{token}"))
    };
    let rewritten = rewrite_column(table, "blocked_by", |value| {
        rewrite_list(value, &replacement)
    });
    rewrite_column(&rewritten, "see_also", |value| {
        rewrite_see_also(value, &replacement)
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
    let replacement = |token: &str| {
        let encoded = token.strip_prefix(UUID_MARKER)?;
        let (uuid, fallback) = encoded.split_once(DISPLAY_FALLBACK_MARKER)?;
        Some(
            uuid_to_display
                .get(uuid)
                .cloned()
                .unwrap_or_else(|| fallback.to_owned()),
        )
    };
    let rewritten = rewrite_column(table, "blocked_by", |value| {
        rewrite_list(value, &replacement)
    });
    rewrite_column(&rewritten, "see_also", |value| {
        rewrite_see_also(value, &replacement)
    })
}

fn rewrite_column(table: &Table, column: &str, rewrite: impl Fn(&str) -> String) -> Table {
    let Some(index) = table.column(column) else {
        return table.clone();
    };
    let mut rewritten = table.clone();
    for row in rewritten.rows.values_mut() {
        let Some(value) = row.get_mut(index) else {
            continue;
        };
        *value = rewrite(value);
    }
    rewritten
}

fn rewrite_see_also(value: &str, replacement: &impl Fn(&str) -> Option<String>) -> String {
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        let tail = &value[index..];
        if tail.starts_with("https://") || tail.starts_with("http://") {
            let length = tail.find(char::is_whitespace).unwrap_or(tail.len());
            output.push_str(&tail[..length]);
            index += length;
            continue;
        }
        if let Some(length) = encoded_reference_len(tail) {
            let token = &tail[..length];
            output.push_str(&replacement(token).unwrap_or_else(|| token.to_owned()));
            index += length;
            continue;
        }
        if let Some(length) = display_reference_len(value, index) {
            let token = &value[index..index + length];
            output.push_str(&replacement(token).unwrap_or_else(|| token.to_owned()));
            index += length;
            continue;
        }
        let character = tail.chars().next().expect("index is before string end");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn encoded_reference_len(value: &str) -> Option<usize> {
    let encoded = value.strip_prefix(UUID_MARKER)?;
    let marker = encoded.find(DISPLAY_FALLBACK_MARKER)?;
    let uuid = &encoded[..marker];
    uuid::Uuid::parse_str(uuid).ok()?;
    let display_start = UUID_MARKER.len() + marker + DISPLAY_FALLBACK_MARKER.len();
    let display_length = display_reference_len(value, display_start)?;
    Some(display_start + display_length)
}

fn display_reference_len(value: &str, start: usize) -> Option<usize> {
    let bytes = value.as_bytes();
    let prefix = *bytes.get(start)?;
    if !matches!(prefix, b'T' | b'H') {
        return None;
    }
    if start > 0 && is_reference_word_byte(bytes[start - 1]) {
        return None;
    }
    let mut end = start + 1;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == start + 1
        || bytes
            .get(end)
            .is_some_and(|byte| is_reference_word_byte(*byte))
    {
        return None;
    }
    Some(end - start)
}

fn is_reference_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
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
