use anyhow::{Result, bail};
use serde_json::{Map, Value};

pub(super) fn parse_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::from(raw))
}

pub(super) fn path_segments(path: &str) -> Result<Vec<&str>> {
    let segments = path.split('.').collect::<Vec<_>>();
    if path.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        bail!("invalid env path `{path}`; use dot-separated names")
    }
    Ok(segments)
}

pub(super) fn get_path<'a>(map: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut current: Option<&'a Value> = None;
    for segment in path_segments(path).ok()? {
        let value = match current {
            None => map.get(segment)?,
            Some(Value::Object(object)) => object.get(segment)?,
            Some(Value::Array(array)) => array.get(segment.parse::<usize>().ok()?)?,
            Some(_) => return None,
        };
        current = Some(value);
    }
    current
}

/// Write `value` at a dotted env path, creating missing intermediate objects.
///
/// Descends through objects by name and through arrays by index, mirroring
/// [`get_path`], so one element of a structured list (`skill_sessions.0.prompt`)
/// is addressable like any nested field. Missing object keys are created; a
/// missing *array* index is an error rather than a silently invented entry.
pub(super) fn set_path(map: &mut Map<String, Value>, path: &str, value: Value) -> Result<()> {
    let segments = path_segments(path)?;
    let (last, parents) = segments.split_last().expect("path has a segment");
    let mut current = map
        .entry((*parents.first().unwrap_or(last)).to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if parents.is_empty() {
        map.insert((*last).to_owned(), value);
        return Ok(());
    }
    for (depth, segment) in parents.iter().enumerate().skip(1) {
        current = descend(current, segment, &segments[..=depth])?;
    }
    match current {
        Value::Object(object) => {
            object.insert((*last).to_owned(), value);
            Ok(())
        }
        Value::Array(array) => {
            let index = array_index(last, array.len(), &segments)?;
            array[index] = value;
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "cannot descend through non-object env value `{}`",
            parents.join(".")
        )),
    }
}

/// One step of [`set_path`]'s walk, creating a missing object key. `walked` is
/// the path so far, for an error a user can locate.
fn descend<'a>(current: &'a mut Value, segment: &str, walked: &[&str]) -> Result<&'a mut Value> {
    match current {
        Value::Array(array) => {
            let index = array_index(segment, array.len(), walked)?;
            Ok(&mut array[index])
        }
        Value::Object(object) => Ok(object
            .entry(segment.to_owned())
            .or_insert_with(|| Value::Object(Map::new()))),
        _ => Err(anyhow::anyhow!(
            "cannot descend through non-object env value `{segment}`"
        )),
    }
}

pub(super) fn array_index(segment: &str, len: usize, walked: &[&str]) -> Result<usize> {
    segment
        .parse::<usize>()
        .ok()
        .filter(|index| *index < len)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no env array element at `{}`; the list holds {len} entr{}",
                walked.join("."),
                if len == 1 { "y" } else { "ies" }
            )
        })
}

pub(in crate::env) fn flatten_map(map: &Map<String, Value>) -> Vec<(String, Value)> {
    let mut rows = Vec::new();
    for (name, value) in map {
        flatten_value(name, value, &mut rows);
    }
    rows
}

pub(super) fn flatten_value(path: &str, value: &Value, rows: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(object) if !object.is_empty() => {
            for (name, value) in object {
                flatten_value(&format!("{path}.{name}"), value, rows);
            }
        }
        Value::Array(array) if !array.is_empty() => {
            for (index, value) in array.iter().enumerate() {
                flatten_value(&format!("{path}.{index}"), value, rows);
            }
        }
        _ => rows.push((path.to_owned(), value.clone())),
    }
}
