//! `brain contacts` — the local contacts book.
//!
//! A single CSV per workspace, and the only correct way to mutate it: id
//! assignment, timestamps, quoting, and field validation all live here so the
//! data stays clean no matter who is typing.
//!
//! Being native also makes it **workspace-scoped**. The script this replaced
//! resolved `~/brain` directly, so on a machine with more than one workspace it
//! read and wrote the wrong book.

pub(crate) mod find;
pub(crate) mod model;
pub(crate) mod render;

#[cfg(test)]
mod tests;

use std::path::Path;

use anyhow::{Result, bail};
use chrono::NaiveDate;

use crate::tasks::complete::Row;
use model::{Contact, PREFERRED_COMMS};

/// The field values an add or edit supplies. `None` means "leave alone".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Fields {
    pub(crate) name: Option<String>,
    pub(crate) job: Option<String>,
    pub(crate) company: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) phone: Option<String>,
    pub(crate) preferred_comms: Option<String>,
    pub(crate) address: Option<String>,
    pub(crate) tags: Option<String>,
    pub(crate) birthday: Option<String>,
    pub(crate) notes: Option<String>,
}

impl Fields {
    fn pairs(&self) -> Vec<(&'static str, &String)> {
        [
            ("name", self.name.as_ref()),
            ("job", self.job.as_ref()),
            ("company", self.company.as_ref()),
            ("email", self.email.as_ref()),
            ("phone", self.phone.as_ref()),
            ("preferred_comms", self.preferred_comms.as_ref()),
            ("address", self.address.as_ref()),
            ("tags", self.tags.as_ref()),
            ("birthday", self.birthday.as_ref()),
            ("notes", self.notes.as_ref()),
        ]
        .into_iter()
        .filter_map(|(column, value)| value.map(|value| (column, value)))
        .collect()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pairs().is_empty()
    }

    /// Normalize and reject anything the schema does not accept.
    fn validated(&self) -> Result<Vec<(&'static str, String)>> {
        self.pairs()
            .into_iter()
            .map(|(column, value)| {
                if column != "preferred_comms" {
                    return Ok((column, value.clone()));
                }
                let normalized = value.trim().to_lowercase();
                if normalized.is_empty() || PREFERRED_COMMS.contains(&normalized.as_str()) {
                    Ok((column, normalized))
                } else {
                    bail!(
                        "--preferred-comms must be one of {}, got '{value}'",
                        PREFERRED_COMMS.join(", ")
                    )
                }
            })
            .collect()
    }
}

/// What a mutation did, for the JSON report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mutation {
    pub(crate) action: &'static str,
    pub(crate) contact: Contact,
}

pub(crate) fn add(root: &Path, fields: &Fields, today: NaiveDate) -> Result<Mutation> {
    let validated = fields.validated()?;
    if fields
        .name
        .as_ref()
        .is_none_or(|name| name.trim().is_empty())
    {
        bail!("--name is required to add a contact");
    }
    let mut rows = model::load(root)?;
    let mut row: Row = model::COLUMNS
        .iter()
        .map(|column| ((*column).to_owned(), String::new()))
        .collect();
    for (column, value) in validated {
        row.insert(column.to_owned(), value);
    }
    row.insert("id".to_owned(), model::next_id(&rows));
    row.insert("created_date".to_owned(), today.to_string());
    row.insert("last_updated".to_owned(), today.to_string());
    let contact = Contact::from_row(&row);
    rows.push(row);
    model::save(root, &rows)?;
    Ok(Mutation {
        action: "added",
        contact,
    })
}

pub(crate) fn edit(
    root: &Path,
    needle: &str,
    fields: &Fields,
    today: NaiveDate,
) -> Result<Mutation> {
    if fields.is_empty() {
        bail!("no fields given to edit (pass at least one field flag)");
    }
    let validated = fields.validated()?;
    let mut rows = model::load(root)?;
    let (index, _) = find::resolve(&rows, needle)?;
    let row = &mut rows[index];
    for (column, value) in validated {
        row.insert(column.to_owned(), value);
    }
    row.insert("last_updated".to_owned(), today.to_string());
    let contact = Contact::from_row(row);
    model::save(root, &rows)?;
    Ok(Mutation {
        action: "edited",
        contact,
    })
}

pub(crate) fn delete(root: &Path, needle: &str) -> Result<Mutation> {
    let mut rows = model::load(root)?;
    let (index, _) = find::resolve(&rows, needle)?;
    let contact = Contact::from_row(&rows[index]);
    rows.remove(index);
    model::save(root, &rows)?;
    Ok(Mutation {
        action: "deleted",
        contact,
    })
}

pub(crate) fn get(root: &Path, needle: &str) -> Result<Contact> {
    let rows = model::load(root)?;
    let (index, _) = find::resolve(&rows, needle)?;
    Ok(Contact::from_row(&rows[index]))
}

pub(crate) fn list(root: &Path, tag: Option<&str>, job: Option<&str>) -> Result<Vec<Contact>> {
    Ok(find::filter(&model::load(root)?, tag, job))
}

pub(crate) fn search(root: &Path, query: &str, only: Option<&str>) -> Result<Vec<Contact>> {
    Ok(find::search(&model::load(root)?, query, only))
}

/// The configured external fallback directory, when one is set.
///
/// Core knows only that the key exists and carries opaque JSON: what service it
/// names, and how to reach it, is the caller's business.
pub(crate) fn fallback(root: &Path) -> Result<serde_json::Value> {
    let path = model::config_path(root);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let fallback = value.get("notion_fallback").cloned().unwrap_or_default();
    if fallback.is_null() || fallback.as_object().is_some_and(serde_json::Map::is_empty) {
        bail!("no fallback directory configured in {}", path.display());
    }
    Ok(fallback)
}
