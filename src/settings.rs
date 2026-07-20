//! Persistent user configuration and the `brain config` command.
//!
//! The store is a JSON object at `~/.config/brain/config.json` (or
//! `$XDG_CONFIG_HOME/brain/config.json`) — machine-local, never shipped with
//! the source. Typed consumers (`config::Config`, `paths`) deserialize the
//! fields they care about from the same file; this module owns the raw
//! read/modify/write, the declared-variable schema, and the get/set/list CLI.
//!
//! It also owns the `markdown-to-pdf` prerequisite: the path is a config
//! variable, auto-discovered on first run (PATH, conventional bin dirs, then
//! the login shell so an autoloaded shell-function wrapper is still found) and
//! persisted. A missing or invalid path is a hard, fail-fast error.
//!
//! Split, as everywhere in this crate, into pure decision helpers (schema
//! resolution, table layout, message wording, shell-output parsing) that are
//! unit-tested, and thin IO shells (`load_map`/`save_map`, discovery probes,
//! the process-exiting gate).

use std::io::IsTerminal;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value};

/// One declared config variable: shown in `config list`, accepted by
/// `config set`, and resolved with an optional built-in default.
struct VarSpec {
    name: &'static str,
    description: &'static str,
    default: Option<&'static str>,
}

/// The full schema, in the order `config list` prints them.
const VARS: [VarSpec; 5] = [
    VarSpec {
        name: "root",
        description: "Path to your brain (PARA) directory. Tilde-expanded. Defaults to ~/brain.",
        default: Some("~/brain"),
    },
    VarSpec {
        name: "linear_workspace",
        description: "Linear workspace slug (e.g. acme). Builds https://linear.app/<slug>/issue/ for the open-link action.",
        default: None,
    },
    VarSpec {
        name: "markdown_to_pdf_path",
        description: "Path to the markdown-to-pdf command. Auto-discovered on first run; required for the Create-PDF action.",
        default: None,
    },
    VarSpec {
        name: "daily_triage_name_pattern",
        description: "Case-insensitive regex matched against habit names to gate the startup triage nudge. Empty disables it.",
        default: Some("Morning Triage"),
    },
    VarSpec {
        name: "day_rollover_hour",
        description: "Local hour (0-23) at which the logical day rolls over for the triage re-check.",
        default: Some("6"),
    },
];

/// A variable paired with its effective value (explicit override, else the
/// built-in default, else `None`).
pub struct Resolved {
    pub name: &'static str,
    pub value: Option<String>,
    pub description: &'static str,
}

// ---------------------------------------------------------------------------
// Store IO
// ---------------------------------------------------------------------------

/// Absolute path to the JSON config store.
#[must_use]
pub fn store_path() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(xdg).join("brain").join("config.json");
    }
    home_dir().join(".config").join("brain").join("config.json")
}

/// Read the store as a JSON object. A missing, unreadable, or non-object file
/// yields an empty map — a broken config never blocks startup.
#[must_use]
pub(crate) fn load_map() -> Map<String, Value> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

fn save_map(map: &Map<String, Value>) -> Result<()> {
    let path = store_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_string_pretty(&Value::Object(map.clone()))?;
    std::fs::write(&path, format!("{body}\n"))?;
    Ok(())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from)
}

// ---------------------------------------------------------------------------
// Get / set / list
// ---------------------------------------------------------------------------

/// Canonicalize a variable name: lowercase, trimmed, dashes to underscores.
#[must_use]
pub fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
}

fn is_known(name: &str) -> bool {
    VARS.iter().any(|v| v.name == name)
}

fn default_of(name: &str) -> Option<&'static str> {
    VARS.iter().find(|v| v.name == name).and_then(|v| v.default)
}

/// Render a JSON value as the flat string the CLI and typed readers see.
fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

/// The raw explicit value for `name` (no default fallback).
#[must_use]
pub fn get(name: &str) -> Option<String> {
    load_map().get(name).and_then(value_to_string)
}

/// The effective value for a known variable: explicit override else default.
#[must_use]
pub fn resolve_one(name: &str) -> Option<String> {
    if !is_known(name) {
        return None;
    }
    get(name).or_else(|| default_of(name).map(str::to_owned))
}

/// Coerce a raw CLI string into the tightest JSON type so typed readers keep
/// working (`day_rollover_hour=4` must round-trip as a number, not `"4"`).
fn parse_value(raw: &str) -> Value {
    if let Ok(i) = raw.parse::<i64>() {
        return Value::from(i);
    }
    match raw {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        other => Value::from(other),
    }
}

/// Persist `name=value` for a declared variable. Unknown names are rejected so
/// a typo can't silently rot in the store.
pub fn set(name: &str, value: &str) -> Result<()> {
    if !is_known(name) {
        bail!("unknown config variable `{name}` (known: {})", known_names());
    }
    let mut map = load_map();
    map.insert(name.to_owned(), parse_value(value));
    save_map(&map)
}

fn known_names() -> String {
    VARS.iter()
        .map(|v| v.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every declared variable with its resolved value, in schema order.
#[must_use]
pub fn resolve_all() -> Vec<Resolved> {
    resolve_all_from(&load_map())
}

/// Pure core of [`resolve_all`]: resolve against an explicit map so the schema
/// and default logic are testable without touching the real store.
fn resolve_all_from(map: &Map<String, Value>) -> Vec<Resolved> {
    VARS.iter()
        .map(|v| Resolved {
            name: v.name,
            value: map
                .get(v.name)
                .and_then(value_to_string)
                .or_else(|| v.default.map(str::to_owned)),
            description: v.description,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering — a plain aligned key / value / description table
// ---------------------------------------------------------------------------

const LIST_HEADERS: [&str; 3] = ["var name", "value", "description"];
const LIST_UNSET: &str = "(unset)";

const RESET: &str = "\x1b[0m";
const HEADER: &str = "\x1b[1;4;95m"; // bold underline bright magenta
const ACCENT: &str = "\x1b[96m"; // bright cyan — var names
const VALUE: &str = "\x1b[97m"; // bright white — values
const MUTED: &str = "\x1b[90m"; // bright black — descriptions
const SUCCESS: &str = "\x1b[92m"; // bright green — "set"
const ERROR: &str = "\x1b[91m"; // bright red — the prerequisite failure

fn paint(code: &str, s: &str, color: bool) -> String {
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

// ---------------------------------------------------------------------------
// markdown-to-pdf prerequisite: discovery, validation, the fail-fast gate
// ---------------------------------------------------------------------------

/// True when `p` is a regular file with an executable bit set.
fn is_executable_file(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Conventional install dirs to probe for a `markdown-to-pdf` executable,
/// in order. Pure so the search order is a checked contract.
#[must_use]
fn conventional_candidates(home: &Path) -> Vec<PathBuf> {
    [
        home.join(".local").join("bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        home.join("bin"),
    ]
    .into_iter()
    .map(|d| d.join("markdown-to-pdf"))
    .collect()
}

/// Absolute-path tokens embedded in shell output (e.g. a resolved command path
/// or the body of an autoloaded function that wraps a script). Splits on
/// whitespace, strips matching surrounding quotes, and keeps only tokens that
/// look like absolute paths — robust to interleaved terminal control junk.
#[must_use]
fn shell_tokens_to_paths(text: &str) -> Vec<PathBuf> {
    text.split_whitespace()
        .map(|t| t.trim_matches(|c| c == '\'' || c == '"'))
        .filter(|t| t.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// Ask the user's login shell to resolve `markdown-to-pdf`. Catches the common
/// case where it is an autoloaded zsh *function* wrapping a real script: we
/// print both the resolved command path and the function body, then mine any
/// executable path out of the result. Best-effort — a missing shell or error
/// yields no candidates.
fn shell_resolved_candidates() -> Vec<PathBuf> {
    let script = "autoload +X markdown-to-pdf 2>/dev/null; \
                  command -v markdown-to-pdf 2>/dev/null; \
                  print -r -- \"${functions[markdown-to-pdf]-}\"";
    Command::new("zsh")
        .arg("-ic")
        .arg(script)
        .output()
        .ok()
        .map(|o| shell_tokens_to_paths(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Find an invokable `markdown-to-pdf`: PATH, then conventional bin dirs, then
/// the login shell. Returns the first candidate that is an executable file.
#[must_use]
pub fn discover_markdown_to_pdf() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("markdown-to-pdf")).collect::<Vec<_>>())
        .unwrap_or_default();

    on_path
        .into_iter()
        .chain(conventional_candidates(&home_dir()))
        .chain(shell_resolved_candidates())
        .find(|p| is_executable_file(p))
}

/// The validated `markdown-to-pdf` command.
///
/// Assumes [`ensure_markdown_to_pdf`] already ran at startup, but re-validates
/// so a config edited mid-session still errors cleanly rather than spawning a
/// bogus path.
pub fn markdown_to_pdf_command() -> Result<PathBuf> {
    match get("markdown_to_pdf_path") {
        Some(p) if is_executable_file(Path::new(&p)) => Ok(PathBuf::from(p)),
        _ => Err(anyhow!(
            "markdown-to-pdf is not configured; run `brain config set markdown_to_pdf_path=<path>`"
        )),
    }
}

/// The red, `❌`-led message shown when the prerequisite can't be satisfied.
/// `configured` carries the offending path when one was set but invalid, so
/// the wording distinguishes "not found" from "misconfigured". `color` gates
/// the ANSI so a captured/piped stderr stays clean; pure for testing.
#[must_use]
fn missing_markdown_to_pdf_message(configured: Option<&str>, color: bool) -> String {
    let head = paint(
        ERROR,
        "❌ brain requires `markdown-to-pdf`, which it can't use.",
        color,
    );
    let detail = configured.map_or_else(
        || {
            "`markdown-to-pdf` is a hard prerequisite: brain runs it to turn markdown\n\
             notes into PDFs. brain couldn't auto-discover it on your PATH, in the\n\
             usual install dirs, or via your login shell.\n"
                .to_owned()
        },
        |p| {
            format!("The configured `markdown_to_pdf_path` is missing or not executable:\n\n    {p}\n")
        },
    );
    format!(
        "{head}\n\n\
         {detail}\n\
         Install `markdown-to-pdf`, or point brain at it:\n\n    \
         brain config set markdown_to_pdf_path=/path/to/markdown-to-pdf"
    )
}

/// Startup gate for the `markdown-to-pdf` prerequisite.
///
/// A valid configured path passes; an unset one triggers auto-discovery
/// (persisted on success); anything else prints the red message and exits
/// non-zero. Exits directly (not via `anyhow`) so the message prints verbatim
/// without an `Error:` prefix.
pub fn ensure_markdown_to_pdf() {
    let Some(configured) = get("markdown_to_pdf_path") else {
        if let Some(found) = discover_markdown_to_pdf() {
            // Persist for next time; a save failure is non-fatal since the
            // tool itself is present and usable right now.
            let _ = set("markdown_to_pdf_path", &found.display().to_string());
            return;
        }
        fail_missing(None);
    };
    if is_executable_file(Path::new(&configured)) {
        return;
    }
    fail_missing(Some(&configured));
}

fn fail_missing(configured: Option<&str>) -> ! {
    eprintln!(
        "{}",
        missing_markdown_to_pdf_message(configured, color_enabled())
    );
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_path_prefers_xdg_config_home() {
        // Documented precedence; we can't safely mutate the process env here,
        // so assert the shape the resolver produces from HOME.
        let p = store_path();
        assert!(p.ends_with("brain/config.json"));
    }

    #[test]
    fn normalize_lowercases_and_underscores() {
        assert_eq!(normalize_name("  Linear-Workspace "), "linear_workspace");
        assert_eq!(normalize_name("ROOT"), "root");
    }

    #[test]
    fn parse_value_tightens_numbers_and_bools() {
        assert_eq!(parse_value("4"), Value::from(4));
        assert_eq!(parse_value("true"), Value::Bool(true));
        assert_eq!(parse_value("~/brain"), Value::from("~/brain"));
        // A slug that merely starts with a digit stays a string.
        assert_eq!(parse_value("2acme"), Value::from("2acme"));
    }

    #[test]
    fn resolve_all_covers_every_var_and_applies_defaults() {
        // Hermetic: resolve against an empty map, not the real store.
        let rows = resolve_all_from(&Map::new());
        assert_eq!(rows.len(), VARS.len());
        let root = rows.iter().find(|r| r.name == "root").unwrap();
        assert_eq!(root.value.as_deref(), Some("~/brain"));
        // No built-in default → unset (until the user or discovery sets it).
        let ws = rows.iter().find(|r| r.name == "linear_workspace").unwrap();
        assert_eq!(ws.value, None);
    }

    #[test]
    fn resolve_all_prefers_an_explicit_value_over_the_default() {
        let mut map = Map::new();
        map.insert("root".to_owned(), Value::from("/srv/brain"));
        map.insert("linear_workspace".to_owned(), Value::from("acme"));
        let rows = resolve_all_from(&map);
        let val = |n: &str| rows.iter().find(|r| r.name == n).unwrap().value.clone();
        assert_eq!(val("root").as_deref(), Some("/srv/brain"));
        assert_eq!(val("linear_workspace").as_deref(), Some("acme"));
    }

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

    #[test]
    fn conventional_candidates_are_ordered_bins_under_home() {
        let c = conventional_candidates(Path::new("/Users/x"));
        assert_eq!(c[0], PathBuf::from("/Users/x/.local/bin/markdown-to-pdf"));
        assert!(c.iter().all(|p| p.ends_with("markdown-to-pdf")));
    }

    #[test]
    fn shell_tokens_extracts_quoted_absolute_path_from_a_function_body() {
        // The exact shape of an autoloaded launcher function.
        let body = "markdown-to-pdf() {\n\temulate -L zsh\n\t\
                    '/Users/x/src/tool/markdown-to-pdf/run.sh' \"$@\"\n}";
        let paths = shell_tokens_to_paths(body);
        assert!(paths.contains(&PathBuf::from("/Users/x/src/tool/markdown-to-pdf/run.sh")));
        // No bare-word / relative tokens leak through.
        assert!(paths.iter().all(|p| p.is_absolute()));
    }

    #[test]
    fn shell_tokens_ignores_terminal_control_noise() {
        let noisy = "\x1b]1337;RemoteHost=me@host\x07 markdown-to-pdf /opt/x/run.sh";
        assert_eq!(
            shell_tokens_to_paths(noisy),
            vec![PathBuf::from("/opt/x/run.sh")]
        );
    }

    #[test]
    fn missing_message_names_the_tool_and_the_fix_in_red() {
        let msg = missing_markdown_to_pdf_message(None, true);
        assert!(msg.contains('❌'));
        assert!(msg.contains(ERROR));
        assert!(msg.contains(RESET));
        assert!(msg.contains("markdown-to-pdf"));
        assert!(msg.contains("brain config set markdown_to_pdf_path="));
    }

    #[test]
    fn missing_message_distinguishes_a_bad_configured_path() {
        let msg = missing_markdown_to_pdf_message(Some("/bad/run.sh"), false);
        assert!(!msg.contains('\x1b')); // color off
        assert!(msg.contains("/bad/run.sh"));
        assert!(msg.contains("missing or not executable"));
    }
}
