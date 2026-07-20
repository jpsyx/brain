//! Rendering the `config list` table and the `config set` confirmation: the
//! ANSI palette, the `color_enabled` gate, the aligned key/value/description
//! layout, and the painting. All pure (given a `color` flag) so it's testable.

use std::io::IsTerminal;

use super::schema::Resolved;

const LIST_HEADERS: [&str; 3] = ["var name", "value", "description"];
const LIST_UNSET: &str = "(unset)";

pub(super) const RESET: &str = "\x1b[0m";
const HEADER: &str = "\x1b[1;4;95m"; // bold underline bright magenta
const ACCENT: &str = "\x1b[96m"; // bright cyan — var names
const VALUE: &str = "\x1b[97m"; // bright white — values
const MUTED: &str = "\x1b[90m"; // bright black — descriptions
const SUCCESS: &str = "\x1b[92m"; // bright green — "set"
pub(super) const ERROR: &str = "\x1b[91m"; // bright red — the prerequisite failure

pub(super) fn paint(code: &str, s: &str, color: bool) -> String {
    if color {
        format!("{code}{s}{RESET}")
    } else {
        s.to_owned()
    }
}

/// Whether to emit ANSI escapes.
///
/// brain's *stdout* is captured by the shell wrapper (so it is never a TTY);
/// the wrapper reprints the bytes verbatim to the terminal. So terminal-ness
/// is judged from stderr, and `NO_COLOR` is honored.
#[must_use]
pub fn color_enabled() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

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
            r.description.to_owned(),
        ]);
    }
    cells
}

/// The full `config list` table as one printable string: a bold-underline
/// header row, then data rows painted cyan (name) / white (value) / dim
/// (description).
#[must_use]
pub fn render_list(rows: &[Resolved], color: bool) -> String {
    let cells = list_table_cells(rows);
    let mut iter = cells.iter();
    let mut lines: Vec<String> = Vec::with_capacity(cells.len());
    if let Some([n, v, d]) = iter.next() {
        lines.push(format!(
            "{}  {}  {}",
            paint(HEADER, n, color),
            paint(HEADER, v, color),
            paint(HEADER, d, color)
        ));
    }
    for [n, v, d] in iter {
        lines.push(format!(
            "{}  {}  {}",
            paint(ACCENT, n, color),
            paint(VALUE, v, color),
            paint(MUTED, d, color)
        ));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// The one-line confirmation `config set` prints.
#[must_use]
pub fn set_confirmation(name: &str, value: &str, color: bool) -> String {
    format!("{} {name} = {value}", paint(SUCCESS, "set", color))
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
        let out = render_list(&rows, true);
        assert!(out.contains(HEADER)); // header row painted
        assert!(out.contains(ACCENT)); // a var name painted
        assert!(out.contains("var name"));
        assert!(out.contains("(unset)")); // linear_workspace has no default
    }

    #[test]
    fn render_list_is_plain_without_color() {
        let rows = resolve_all_from(&Map::new());
        let out = render_list(&rows, false);
        assert!(!out.contains('\x1b'));
        assert!(out.contains("var name"));
    }

    #[test]
    fn set_confirmation_greens_the_verb() {
        assert!(set_confirmation("root", "~/b", true).contains(SUCCESS));
        assert_eq!(set_confirmation("root", "~/b", false), "set root = ~/b");
    }
}
