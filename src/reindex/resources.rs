//! Pure `zotero-lookup.csv` row derivation from a resource `.METADATA.json`
//! plus the colocated `notes.md` flags.
//!
//! Resource metadata is heterogeneous — it was written by several generations
//! of import tooling. `year` may be a JSON string *or* number, the item type
//! may be keyed `item_type` *or* `type`, and optional fields may be absent or
//! `null`. Parsing therefore goes through `serde_json::Value` with defensive
//! coercion so a valid-but-differently-shaped record is never silently dropped.

use serde_json::Value;

use super::notes::NotesFlags;

/// The `zotero-lookup.csv` column order.
pub const RESOURCES_HEADER: [&str; 13] = [
    "zotero_key",
    "title",
    "authors",
    "year",
    "item_type",
    "collection",
    "directory",
    "has_pdf",
    "has_html",
    "has_summary",
    "has_other_notes",
    "annotation_count",
    "tags",
];

/// The subset of a resource `.METADATA.json` that feeds the lookup CSV.
#[derive(Debug, Default)]
pub struct ResourceMeta {
    pub zotero_key: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: String,
    pub item_type: String,
    pub collection: String,
    pub tags: Vec<String>,
    pub attachment_kinds: Vec<String>,
}

/// Parse a resource `.METADATA.json`; `None` only if it isn't valid JSON.
#[must_use]
pub fn parse_resource_meta(json: &str) -> Option<ResourceMeta> {
    let value: Value = serde_json::from_str(json).ok()?;
    Some(ResourceMeta {
        zotero_key: scalar(&value, "zotero_key"),
        title: scalar(&value, "title"),
        authors: string_list(&value, "authors"),
        year: scalar(&value, "year"),
        item_type: first_present(&value, &["item_type", "type"]),
        collection: scalar(&value, "collection"),
        tags: string_list(&value, "tags"),
        attachment_kinds: attachment_kinds(&value),
    })
}

/// Build one lookup row. `directory` is the brain-root-relative path; `flags`
/// are the scanned `notes.md` results.
#[must_use]
pub fn resource_row(meta: &ResourceMeta, directory: &str, flags: &NotesFlags) -> Vec<String> {
    vec![
        meta.zotero_key.clone(),
        meta.title.clone(),
        meta.authors.join(";"),
        meta.year.clone(),
        meta.item_type.clone(),
        meta.collection.clone(),
        directory.to_owned(),
        yes_no(meta.attachment_kinds.iter().any(|k| k == "pdf")),
        yes_no(meta.attachment_kinds.iter().any(|k| k == "html")),
        yes_no(flags.has_summary),
        yes_no(flags.has_other_notes),
        flags.annotation_count.to_string(),
        meta.tags.join(";"),
    ]
}

/// A scalar field coerced to a string: strings verbatim, numbers/bools
/// stringified, anything else (missing / null / container) an empty string.
fn scalar(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// The first of several candidate keys that yields a non-empty scalar.
fn first_present(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|k| scalar(value, k))
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

/// A string array, coercing scalar elements; empty when absent or not an array.
fn string_list(value: &Value, key: &str) -> Vec<String> {
    let Some(Value::Array(items)) = value.get(key) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

/// The `type` of each attachment object under `attachments`.
fn attachment_kinds(value: &Value) -> Vec<String> {
    let Some(Value::Array(items)) = value.get("attachments") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item.get("type").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_authors_and_tags_and_reads_attachment_kinds() {
        let json = r#"{
            "zotero_key": "GB2QL5W4",
            "title": "Rejection sensitivity and social outcomes",
            "authors": ["Canu, Will H.", "Carlson, Caryn L."],
            "year": "2007",
            "item_type": "journalArticle",
            "collection": ".Next;Read up on ADHD",
            "tags": ["unread", "adhd"],
            "attachments": [{"type": "pdf", "filename": "x.pdf"}]
        }"#;
        let meta = parse_resource_meta(json).expect("valid json");
        let flags = NotesFlags {
            has_summary: true,
            has_other_notes: false,
            annotation_count: 0,
        };
        let row = resource_row(&meta, "resources/adhd/rejection-sensitivity", &flags);
        assert_eq!(
            row,
            vec![
                "GB2QL5W4",
                "Rejection sensitivity and social outcomes",
                "Canu, Will H.;Carlson, Caryn L.",
                "2007",
                "journalArticle",
                ".Next;Read up on ADHD",
                "resources/adhd/rejection-sensitivity",
                "yes", // has_pdf
                "no",  // has_html
                "yes", // has_summary
                "no",  // has_other_notes
                "0",   // annotation_count
                "unread;adhd",
            ]
        );
    }

    #[test]
    fn coerces_numeric_year_and_falls_back_from_item_type_to_type() {
        // The "reference_material" schema variant: numeric year, `type` key,
        // no `collection`. Must not be skipped.
        let json = r#"{
            "zotero_key": "2YMLKIGH",
            "title": "The State of Social Enterprise 2024",
            "type": "document",
            "authors": ["World Economic Forum"],
            "year": 2024,
            "date": null,
            "tags": ["notion", "reference"],
            "attachments": [{"type": "pdf", "filename": "x.pdf"}]
        }"#;
        let meta = parse_resource_meta(json).expect("valid json");
        let row = resource_row(&meta, "resources/x/y", &NotesFlags::empty());
        assert_eq!(row[3], "2024"); // numeric year coerced
        assert_eq!(row[4], "document"); // type -> item_type fallback
        assert_eq!(row[5], ""); // no collection
        assert_eq!(row[7], "yes"); // has_pdf
    }

    #[test]
    fn missing_optional_fields_render_empty() {
        let meta = parse_resource_meta(r#"{"zotero_key":"QEUUTVFD","item_type":"webpage"}"#)
            .expect("valid json");
        let row = resource_row(&meta, "resources/x/y", &NotesFlags::empty());
        assert_eq!(row[2], ""); // authors
        assert_eq!(row[3], ""); // year
        assert_eq!(row[7], "no"); // has_pdf
        assert_eq!(row[12], ""); // tags
    }
}
