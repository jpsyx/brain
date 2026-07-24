//! Skill extensions: additive personalization injected into a bundled skill's
//! **built copy** (never the repo source).
//!
//! An extension file (`<root>/.config/extensions/<skill>.md`) is a set of
//! `[hook-name]` sections. The base skill declares matching hook markers
//! (`<!-- brain:ext hook-name -->`); `apply` substitutes each hook's content at
//! its marker. Content before the first `[hook]` (and any hook with no matching
//! marker) is appended as a trailing "Personal extensions" section, so nothing
//! the user wrote is silently dropped.
//!
//! `parse` and `apply` are pure and unit-tested; `load` is the thin file read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MARKER_PREFIX: &str = "<!-- brain:ext ";
const MARKER_SUFFIX: &str = " -->";
const PERSONAL_HEADER: &str = "## Personal extensions";

/// A parsed extension: named hook → content, plus the leading catch-all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extension {
    pub hooks: BTreeMap<String, String>,
    pub catch_all: String,
}

/// Parse an extension file. Lines before the first `[hook]` are the catch-all;
/// each `[hook]` line opens a section that runs to the next `[hook]` or EOF.
#[must_use]
pub fn parse(text: &str) -> Extension {
    let mut hooks: BTreeMap<String, String> = BTreeMap::new();
    let mut catch_all = String::new();
    let mut current: Option<String> = None;
    let mut buf = String::new();

    let flush = |current: &Option<String>, buf: &str, hooks: &mut BTreeMap<String, String>, catch: &mut String| {
        let trimmed = buf.trim();
        match current {
            Some(name) => {
                hooks.insert(name.clone(), trimmed.to_owned());
            }
            None => {
                if !trimmed.is_empty() {
                    catch.push_str(trimmed);
                }
            }
        }
    };

    for line in text.lines() {
        if let Some(name) = section_header(line) {
            flush(&current, &buf, &mut hooks, &mut catch_all);
            buf.clear();
            current = Some(name.to_owned());
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    flush(&current, &buf, &mut hooks, &mut catch_all);

    Extension { hooks, catch_all }
}

/// A `[name]` section header (the whole trimmed line), else `None`.
fn section_header(line: &str) -> Option<&str> {
    let t = line.trim();
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    (!inner.is_empty() && !inner.contains('[') && !inner.contains(']')).then_some(inner)
}

/// The hook name in a `<!-- brain:ext NAME -->` marker line, else `None`.
fn marker_hook(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix(MARKER_PREFIX)?.strip_suffix(MARKER_SUFFIX).map(str::trim)
}

/// Inject `ext` into a base SKILL.md body.
///
/// Markers are replaced by their hook's content (or removed if the hook is
/// absent); the catch-all and any unused hooks are appended under a "Personal
/// extensions" section.
#[must_use]
pub fn apply(skill_md: &str, ext: &Extension) -> String {
    let mut used: BTreeSet<&str> = BTreeSet::new();
    let mut out_lines: Vec<String> = Vec::new();

    for line in skill_md.lines() {
        if let Some(hook) = marker_hook(line) {
            if let Some(content) = ext.hooks.get(hook) {
                used.insert(hook);
                if !content.is_empty() {
                    out_lines.push(content.clone());
                }
            }
            // Marker with no matching hook: drop the marker line entirely.
        } else {
            out_lines.push(line.to_owned());
        }
    }

    let mut out = out_lines.join("\n");

    // Anything not consumed by a marker becomes a trailing personal section.
    let mut leftover = String::new();
    if !ext.catch_all.is_empty() {
        leftover.push_str(&ext.catch_all);
    }
    for (name, content) in &ext.hooks {
        if !used.contains(name.as_str()) && !content.is_empty() {
            if !leftover.is_empty() {
                leftover.push_str("\n\n");
            }
            leftover.push_str(content);
        }
    }
    if !leftover.is_empty() {
        out.push_str("\n\n");
        out.push_str(PERSONAL_HEADER);
        out.push_str("\n\n");
        out.push_str(&leftover);
        out.push('\n');
    }

    out
}

/// Load the extension for `skill_name` from `dir` (`<dir>/<skill_name>.md`), if
/// present and readable.
#[must_use]
pub fn load(skill_name: &str, dir: &Path) -> Option<Extension> {
    let path = dir.join(format!("{skill_name}.md"));
    std::fs::read_to_string(path).ok().map(|t| parse(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_hooks_and_catch_all() {
        let ext = parse("intro line\n[triage:start]\nrun email-triage\n[triage:pdf]\nwrite to ~/Downloads\n");
        assert_eq!(ext.catch_all, "intro line");
        assert_eq!(ext.hooks.get("triage:start").unwrap(), "run email-triage");
        assert_eq!(ext.hooks.get("triage:pdf").unwrap(), "write to ~/Downloads");
    }

    #[test]
    fn apply_substitutes_marker_with_hook_content() {
        let base = "# Triage\n<!-- brain:ext triage:start -->\n## Steps\ndo the thing\n";
        let mut ext = Extension::default();
        ext.hooks.insert("triage:start".to_owned(), "First, run email-triage.".to_owned());
        let out = apply(base, &ext);
        // Injected content replaces the marker, before the Steps section.
        let start = out.find("First, run email-triage.").unwrap();
        let steps = out.find("## Steps").unwrap();
        assert!(start < steps, "hook content must land where the marker was");
        assert!(!out.contains("brain:ext"), "marker must be gone");
    }

    #[test]
    fn apply_drops_markers_with_no_matching_hook() {
        let base = "# X\n<!-- brain:ext x:unused -->\nbody\n";
        let out = apply(base, &Extension::default());
        assert!(!out.contains("brain:ext"));
        assert!(out.contains("body"));
    }

    #[test]
    fn apply_appends_catch_all_as_personal_section() {
        let base = "# X\nbody\n";
        let ext = Extension {
            catch_all: "an extra note".to_owned(),
            ..Extension::default()
        };
        let out = apply(base, &ext);
        assert!(out.contains("## Personal extensions"));
        assert!(out.trim_end().ends_with("an extra note"));
    }

    #[test]
    fn apply_appends_unmatched_hooks_so_nothing_is_lost() {
        let base = "# X\nbody\n"; // no markers at all
        let mut ext = Extension::default();
        ext.hooks.insert("x:nowhere".to_owned(), "orphan content".to_owned());
        let out = apply(base, &ext);
        assert!(out.contains("## Personal extensions"));
        assert!(out.contains("orphan content"));
    }

    #[test]
    fn base_with_no_extension_is_unchanged_apart_from_markers() {
        // An empty extension leaves body intact and strips any markers.
        let base = "# X\nreal content\n";
        assert_eq!(apply(base, &Extension::default()), "# X\nreal content");
    }
}
