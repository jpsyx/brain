//! Rendering the `config list` table and the `config set` confirmation: the
//! aligned key/value/description layout, painted via the shared [`Theme`]
//! tokens. All pure (given a `Theme`) so it's testable.

use crate::theme::Theme;

use super::schema::Resolved;

const LIST_HEADERS: [&str; 3] = ["var name", "value", "description"];
const LIST_UNSET: &str = "(unset)";

/// Whether to emit ANSI escapes. Thin re-export of [`crate::theme::color_enabled`]
/// so `markdown_pdf`'s `super::render::color_enabled` path keeps working.
pub use crate::theme::color_enabled;

/// Build the padded, uncolored cells of the `config list` table: row 0 is the
/// header, the rest one row per variable. `var name` and `value` are padded to
/// their widest cell (header included); `description` is last and unpadded.
fn list_table_cells(rows: &[Resolved]) -> Vec<[String; 3]> {
    let value_of = |r: &Resolved| r.value.clone().unwrap_or_else(|| LIST_UNSET.to_owned());
    let name_w = rows
        .iter()
        .map(|r| r.name.len())
        .chain([LIST_HEADERS[0].len()])
        .max()
        .unwrap_or(0);
    let value_w = rows
        .iter()
        .map(|r| value_of(r).len())
        .chain([LIST_HEADERS[1].len()])
        .max()
        .unwrap_or(0);

    let mut cells = Vec::with_capacity(rows.len() + 1);
    cells.push([
        format!("{:<name_w$}", LIST_HEADERS[0]),
        format!("{:<value_w$}", LIST_HEADERS[1]),
        LIST_HEADERS[2].to_owned(),
    ]);
    for r in rows {
        cells.push([
            format!("{:<name_w$}", r.name),
            format!("{:<value_w$}", value_of(r)),
            r.description.clone(),
        ]);
    }
    cells
}

/// The full `config list` table as one printable string: a heading row, then
/// data rows painted accent (name) / value (value) / muted (description).
#[must_use]
pub fn render_list(rows: &[Resolved], theme: Theme) -> String {
    let cells = list_table_cells(rows);
    let mut iter = cells.iter();
    let mut lines: Vec<String> = Vec::with_capacity(cells.len());
    if let Some([n, v, d]) = iter.next() {
        lines.push(format!("{}  {}  {}", theme.heading(n), theme.heading(v), theme.heading(d)));
    }
    for [n, v, d] in iter {
        lines.push(format!("{}  {}  {}", theme.accent(n), theme.value(v), theme.muted(d)));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// The one-line confirmation `config set` prints.
#[must_use]
pub fn set_confirmation(name: &str, value: &str, theme: Theme) -> String {
    format!("{} {name} = {value}", theme.success("set"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::vars::resolve_all_from;
    use serde_json::Map;

    #[test]
    fn list_table_pads_name_and_value_and_leaves_description() {
        let rows = resolve_all_from(&Map::new());
        let cells = list_table_cells(&rows);
        // Header first.
        assert_eq!(cells[0][0].trim_end(), "var name");
        // Every name/value cell shares a width; description is untouched.
        let nw = cells[0][0].len();
        let vw = cells[0][1].len();
        assert!(cells.iter().all(|r| r[0].len() == nw && r[1].len() == vw));
        assert!(cells[1][2] == rows[0].description);
    }

    #[test]
    fn render_list_paints_header_and_rows_when_colored() {
        let rows = resolve_all_from(&Map::new());
        let theme = Theme::dark(true);
        let out = render_list(&rows, theme);
        assert!(out.contains("\x1b[1;95m")); // header row painted (bold bright magenta)
        assert!(out.contains("\x1b[96m")); // a var name painted accent (bright cyan)
        assert!(out.contains("var name"));
        assert!(out.contains("(unset)")); // linear_workspace has no default
    }

    #[test]
    fn render_list_is_plain_without_color() {
        let rows = resolve_all_from(&Map::new());
        let out = render_list(&rows, Theme::dark(false));
        assert!(!out.contains('\x1b'));
        assert!(out.contains("var name"));
    }

    #[test]
    fn set_confirmation_greens_the_verb() {
        assert!(set_confirmation("root", "~/b", Theme::dark(true)).contains("\x1b[92m"));
        assert_eq!(set_confirmation("root", "~/b", Theme::dark(false)), "set root = ~/b");
    }
}
