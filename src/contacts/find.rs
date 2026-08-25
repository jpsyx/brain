//! Resolving and searching contacts.
//!
//! Resolution is deliberately staged — exact id, then exact name, then a name
//! substring — and **refuses** rather than guesses when more than one contact
//! matches. Editing the wrong person's phone number is not a recoverable
//! mistake if nobody notices.

use anyhow::{Result, bail};

use super::model::{Contact, SEARCH_FIELDS};
use crate::tasks::complete::{Row, field};

/// Where a needle resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Matched {
    Id,
    ExactName,
    NameFragment,
}

/// Find the one contact `needle` names.
pub(crate) fn resolve(rows: &[Row], needle: &str) -> Result<(usize, Matched)> {
    let needle = needle.trim();
    let lowered = needle.to_lowercase();

    if let Some(index) = rows
        .iter()
        .position(|row| field(row, "id").to_lowercase() == lowered)
    {
        return Ok((index, Matched::Id));
    }
    for (kind, matches) in [
        (Matched::ExactName, positions(rows, |name| name == lowered)),
        (
            Matched::NameFragment,
            positions(rows, |name| name.contains(&lowered)),
        ),
    ] {
        match matches.as_slice() {
            [only] => return Ok((*only, kind)),
            [] => {}
            several => bail!(
                "'{needle}' matches multiple contacts; name the id instead:\n{}",
                several
                    .iter()
                    .map(|index| describe(&rows[*index]))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }
    bail!("no contact matches '{needle}' (by id or name)")
}

fn positions(rows: &[Row], predicate: impl Fn(&str) -> bool) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| predicate(&field(row, "name").to_lowercase()))
        .map(|(index, _)| index)
        .collect()
}

fn describe(row: &Row) -> String {
    let job = field(row, "job");
    if job.trim().is_empty() {
        format!("  {}  {}", field(row, "id"), field(row, "name"))
    } else {
        format!("  {}  {}  ({job})", field(row, "id"), field(row, "name"))
    }
}

/// Case-insensitive substring search across one field or every searched field.
pub(crate) fn search(rows: &[Row], query: &str, only: Option<&str>) -> Vec<Contact> {
    let query = query.to_lowercase();
    let fields: Vec<&str> = only.map_or_else(|| SEARCH_FIELDS.to_vec(), |field| vec![field]);
    rows.iter()
        .filter(|row| {
            fields
                .iter()
                .any(|column| field(row, column).to_lowercase().contains(&query))
        })
        .map(Contact::from_row)
        .collect()
}

/// Filter by an exact tag (tags are `;`-separated) and a job substring.
pub(crate) fn filter(rows: &[Row], tag: Option<&str>, job: Option<&str>) -> Vec<Contact> {
    rows.iter()
        .filter(|row| {
            tag.is_none_or(|tag| {
                field(row, "tags")
                    .split(';')
                    .any(|value| value.trim().eq_ignore_ascii_case(tag))
            })
        })
        .filter(|row| {
            job.is_none_or(|job| {
                field(row, "job")
                    .to_lowercase()
                    .contains(&job.to_lowercase())
            })
        })
        .map(Contact::from_row)
        .collect()
}
