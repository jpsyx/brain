//! The `brain personalize` command: show / get / set / edit.
//!
//! The pure decision helpers (`summary_block`, `get_field`, `set_field`) are
//! unit-tested; the IO orchestration (`run_*`) loads/saves the store, opens
//! `$EDITOR`, prints, and triggers a skill re-render, and is kept thin.

use std::process::Command;

use anyhow::{Result, bail};

use super::model::Personalization;
use super::store;
use crate::settings::normalize_name;

/// The identity fields addressable by `personalize get`/`set` (tag styles are
/// edited as raw JSON via `edit`).
const FIELDS: [&str; 3] = ["name", "role", "works_for"];

/// A stable, Claude-readable keyed block: the runtime-lookup target skills read.
///
/// Referenced via `brain personalize show`. Always emits every identity key so a
/// parser sees a fixed shape; unset values render as `(unset)`.
#[must_use]
pub fn summary_block(p: &Personalization) -> String {
    let line = |label: &str, v: &str| {
        let shown = if v.is_empty() { "(unset)" } else { v };
        format!("{label}: {shown}")
    };
    let mut out = String::new();
    out.push_str(&line("name", &p.name));
    out.push('\n');
    out.push_str(&line("role", &p.role));
    out.push('\n');
    out.push_str(&line("works_for", &p.works_for));
    out
}

/// Read one identity field's effective value (empty string reads as `None`).
#[must_use]
pub fn get_field(p: &Personalization, field: &str) -> Option<String> {
    let v = match field {
        "name" => &p.name,
        "role" => &p.role,
        "works_for" => &p.works_for,
        _ => return None,
    };
    (!v.is_empty()).then(|| v.clone())
}

/// Set one identity field. Unknown fields error; `tag_styles` is rejected with a
/// pointer to `edit` (it is structured, not a flat string).
pub fn set_field(p: &mut Personalization, field: &str, value: &str) -> Result<()> {
    match field {
        "name" => value.clone_into(&mut p.name),
        "role" => value.clone_into(&mut p.role),
        "works_for" => value.clone_into(&mut p.works_for),
        "tag_styles" => bail!("tag_styles is structured — edit it with `brain personalize edit`"),
        other => bail!(
            "unknown personalization field `{other}` (known: {})",
            FIELDS.join(", ")
        ),
    }
    Ok(())
}

// --- IO orchestration (thin) ---

/// `brain personalize show` — print the keyed summary block.
pub fn run_show() {
    println!("{}", summary_block(&store::load()));
}

/// `brain personalize get <field>`.
pub fn run_get(raw_field: &str) {
    let field = normalize_name(raw_field);
    match get_field(&store::load(), &field) {
        Some(v) => println!("{v}"),
        None => eprintln!("{field} is unset"),
    }
}

/// `brain personalize set <field>=<value>` — persist, then re-render skills.
pub fn run_set(assignment: &str) -> Result<()> {
    let (raw_field, value) = assignment
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("expected field=value, got `{assignment}`"))?;
    let field = normalize_name(raw_field);
    let mut p = store::load();
    set_field(&mut p, &field, value.trim())?;
    store::save(&p)?;
    crate::skills::resync_skills();
    println!("set {field} = {}", value.trim());
    Ok(())
}

/// `brain personalize edit` — open the raw JSON store in `$EDITOR`, then
/// re-render skills. Ensures the file exists first so the editor opens a real
/// (default-populated) document.
pub fn run_edit() -> Result<()> {
    let existing = store::load();
    store::save(&existing)?; // materialize defaults if absent
    let path = store::path_in_root(&crate::paths::brain_root()?);
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nvim".to_owned());
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} {}", shell_quote(&path.to_string_lossy())))
        .status()?;
    if !status.success() {
        bail!("editor exited with {status}");
    }
    crate::skills::resync_skills();
    Ok(())
}

/// Minimal single-quote shell escaping for the editor path.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Personalization {
        Personalization {
            name: "Pablo".to_owned(),
            role: "CEO".to_owned(),
            works_for: "Avandar".to_owned(),
            ..Personalization::default()
        }
    }

    #[test]
    fn summary_block_emits_stable_keyed_lines() {
        let block = summary_block(&sample());
        assert_eq!(block, "name: Pablo\nrole: CEO\nworks_for: Avandar");
    }

    #[test]
    fn summary_block_shows_unset_for_empty_fields() {
        let block = summary_block(&Personalization::default());
        assert_eq!(block, "name: (unset)\nrole: (unset)\nworks_for: (unset)");
    }

    #[test]
    fn get_field_reads_known_fields_and_none_for_empty_or_unknown() {
        let p = sample();
        assert_eq!(get_field(&p, "role").as_deref(), Some("CEO"));
        assert_eq!(get_field(&Personalization::default(), "role"), None);
        assert_eq!(get_field(&p, "bogus"), None);
    }

    #[test]
    fn set_field_updates_known_fields() {
        let mut p = Personalization::default();
        set_field(&mut p, "role", "student").unwrap();
        set_field(&mut p, "works_for", "myself").unwrap();
        assert_eq!(p.role, "student");
        assert_eq!(p.works_for, "myself");
    }

    #[test]
    fn set_field_rejects_unknown_field() {
        let mut p = Personalization::default();
        assert!(set_field(&mut p, "nope", "x").is_err());
    }

    #[test]
    fn set_field_points_tag_styles_at_edit() {
        let mut p = Personalization::default();
        let err = set_field(&mut p, "tag_styles", "{}").unwrap_err();
        assert!(err.to_string().contains("edit"));
    }
}
