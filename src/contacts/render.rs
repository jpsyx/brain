//! Contact output: JSON by default, a table on request.

use super::model::Contact;

/// Columns the table shows, in order.
const TABLE_COLUMNS: [&str; 6] = ["id", "name", "job", "company", "phone", "email"];

/// Pure: the aligned table.
pub(crate) fn table(contacts: &[Contact]) -> String {
    use std::fmt::Write as _;

    if contacts.is_empty() {
        return "(no matching contacts)\n".to_owned();
    }
    let widths: Vec<usize> = TABLE_COLUMNS
        .iter()
        .map(|column| {
            contacts
                .iter()
                .map(|contact| contact.get(column).chars().count())
                .chain(std::iter::once(column.len()))
                .max()
                .unwrap_or(column.len())
        })
        .collect();
    let mut out = String::new();
    let header = TABLE_COLUMNS
        .iter()
        .zip(&widths)
        .map(|(column, width)| format!("{column:<width$}"))
        .collect::<Vec<_>>()
        .join("  ");
    let _ = writeln!(out, "{}", header.trim_end());
    let _ = writeln!(
        out,
        "{}",
        widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("  ")
    );
    for contact in contacts {
        let line = TABLE_COLUMNS
            .iter()
            .zip(&widths)
            .map(|(column, width)| {
                let value = contact.get(column);
                let padding = width.saturating_sub(value.chars().count());
                format!("{value}{}", " ".repeat(padding))
            })
            .collect::<Vec<_>>()
            .join("  ");
        let _ = writeln!(out, "{}", line.trim_end());
    }
    out
}
