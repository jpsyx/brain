//! The `brain persona` command: show / list / get / set / edit.
//!
//! The pure decision helpers (`summary_block`, `persona_block`, `roster_block`,
//! `get_field`, `set_field`, `validate_user`) are unit-tested; the IO
//! orchestration (`run_*`) loads/saves the store, opens `$EDITOR`, prints, and
//! triggers a skill re-render, and is kept thin.
//!
//! Every read and write names a portable user, so a workspace with several
//! members keeps one persona per person. Omitting the user means this machine's
//! local person.

use std::process::Command;

use anyhow::{Result, bail};

use super::persona::Persona;
use super::personas::Personas;
use super::store;
use crate::settings::normalize_name;
use crate::workspace::WorkspaceContext;

/// The identity fields addressable by `persona get`/`set` (tag styles are
/// edited as raw JSON via `edit`).
const FIELDS: [&str; 3] = ["name", "role", "works_for"];

/// A stable, Claude-readable keyed block: the runtime-lookup target skills read.
///
/// Always emits every identity key so a parser sees a fixed shape; unset values
/// render as `(unset)`.
#[must_use]
pub fn summary_block(p: &Persona) -> String {
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
    out.push('\n');
    // Always the *effective* set (falls back to generic defaults) so a skill
    // reading this contract sees a usable namespace list even before setup.
    out.push_str("namespaces: ");
    out.push_str(&super::namespaces::effective(&p.namespaces).join(", "));
    out
}

/// One member's block, headed by the user ID it belongs to.
///
/// The local person is marked so an agent reading several personas knows which
/// one it is currently assisting.
#[must_use]
pub fn persona_block(user_id: &str, p: &Persona, is_local: bool) -> String {
    let suffix = if is_local { " (this machine)" } else { "" };
    format!("user: {user_id}{suffix}\n{}", summary_block(p))
}

/// Every member's block in stable user-ID order, blank-line separated.
///
/// `roster` is the workspace's portable membership, so a person who has not
/// personalized anything still appears (with `(unset)` values) rather than
/// silently vanishing from what a skill sees. A stored persona whose user has
/// left the roster is still listed, so nothing quietly disappears. Pure.
#[must_use]
pub fn roster_block(personas: &Personas, roster: &[&str], local_user_id: &str) -> String {
    let mut ids = personas.ids();
    ids.extend(roster.iter().map(|id| (*id).to_owned()));
    ids.push(local_user_id.to_owned());
    ids.sort();
    ids.dedup();
    ids.iter()
        .map(|id| persona_block(id, &personas.persona_of(id), id == local_user_id))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Read one identity field's effective value (empty string reads as `None`).
#[must_use]
pub fn get_field(p: &Persona, field: &str) -> Option<String> {
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
pub fn set_field(p: &mut Persona, field: &str, value: &str) -> Result<()> {
    match field {
        "name" => value.clone_into(&mut p.name),
        "role" => value.clone_into(&mut p.role),
        "works_for" => value.clone_into(&mut p.works_for),
        "tag_styles" => bail!(
            "tag_styles is structured — edit it with `{}`",
            crate::workspace::suggest("persona edit")
        ),
        other => bail!(
            "unknown persona field `{other}` (known: {})",
            FIELDS.join(", ")
        ),
    }
    Ok(())
}

/// Reject a user ID the workspace has never heard of, naming the ones it knows.
///
/// A workspace with no portable user store yet (legacy, pre-migration) accepts
/// any ID rather than blocking personalization on an unrelated migration. Pure.
pub fn validate_user(roster: &[String], user_id: &str) -> Result<()> {
    if roster.is_empty() || roster.iter().any(|id| id == user_id) {
        return Ok(());
    }
    bail!(
        "unknown user `{user_id}` (workspace members: {})",
        roster.join(", ")
    )
}

/// Resolve the user a command addresses: the requested ID, else this machine's
/// local person.
fn target_user(workspace: &WorkspaceContext, requested: Option<&str>) -> String {
    requested
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map_or_else(|| workspace.local_user_id().to_owned(), str::to_owned)
}

/// The workspace's portable membership, or an empty roster when the store is
/// missing or unreadable — persona reads must not depend on it.
fn roster(workspace: &WorkspaceContext) -> Vec<String> {
    crate::users::UsersStore::load(workspace).map_or_else(
        |_| Vec::new(),
        |users| users.users.iter().map(|user| user.id.to_string()).collect(),
    )
}

// --- IO orchestration (thin) ---

/// `brain persona show [--user <id>]` — print one member's keyed block.
pub fn run_show(workspace: &WorkspaceContext, user: Option<&str>) -> Result<()> {
    let user_id = target_user(workspace, user);
    validate_user(&roster(workspace), &user_id)?;
    println!(
        "{}",
        persona_block(
            &user_id,
            &store::load_persona(workspace, &user_id),
            user_id == workspace.local_user_id(),
        )
    );
    Ok(())
}

/// `brain persona list` — every member's keyed block, the local one marked.
pub fn run_list(workspace: &WorkspaceContext) {
    let roster = roster(workspace);
    let roster = roster.iter().map(String::as_str).collect::<Vec<_>>();
    println!(
        "{}",
        roster_block(&store::load(workspace), &roster, workspace.local_user_id())
    );
}

/// `brain persona get <user> [<field>]` — everything about one member, or one
/// field of theirs.
pub fn run_get(workspace: &WorkspaceContext, user: &str, field: Option<&str>) -> Result<()> {
    let user_id = target_user(workspace, Some(user));
    validate_user(&roster(workspace), &user_id)?;
    let persona = store::load_persona(workspace, &user_id);
    let Some(field) = field else {
        println!(
            "{}",
            persona_block(&user_id, &persona, user_id == workspace.local_user_id())
        );
        return Ok(());
    };
    let theme = crate::theme::Theme::active();
    let field = normalize_name(field);
    match get_field(&persona, &field) {
        Some(value) => println!("{}", theme.value(&value)),
        None => eprintln!(
            "{}",
            theme.warning(&format!("{field} is unset for {user_id}"))
        ),
    }
    Ok(())
}

/// `brain persona set <field>=<value> [--user <id>]` — persist, then re-render
/// skills.
pub fn run_set(workspace: &WorkspaceContext, user: Option<&str>, assignment: &str) -> Result<()> {
    let (raw_field, value) = assignment
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("expected field=value, got `{assignment}`"))?;
    let user_id = target_user(workspace, user);
    validate_user(&roster(workspace), &user_id)?;
    let field = normalize_name(raw_field);
    let mut persona = store::load_persona(workspace, &user_id);
    set_field(&mut persona, &field, value.trim())?;
    store::save_persona(workspace, &user_id, &persona)?;
    crate::skills::resync_skills(workspace);
    let theme = crate::theme::Theme::active();
    println!(
        "{} {field} = {} for {}",
        theme.success("set"),
        theme.value(value.trim()),
        theme.accent(&user_id)
    );
    Ok(())
}

/// Interactively edit the local person's project namespaces via the
/// toggle-checklist.
///
/// Started with the current set, or the generic defaults if unset. Cancel/no-tty
/// leaves it unchanged. Used by `brain config set namespaces`.
pub fn run_set_namespaces(workspace: &WorkspaceContext) -> Result<()> {
    let user_id = workspace.local_user_id().to_owned();
    let mut persona = store::load_persona(workspace, &user_id);
    let initial = super::namespaces::effective(&persona.namespaces);
    match super::checklist::choose("Project namespaces", &initial, super::namespaces::normalize)? {
        Some(sel) => {
            persona.namespaces = sel;
            store::save_persona(workspace, &user_id, &persona)?;
            crate::skills::resync_skills(workspace);
            println!(
                "namespaces: {}",
                super::namespaces::effective(&persona.namespaces).join(", ")
            );
        }
        None => println!("namespaces unchanged"),
    }
    Ok(())
}

/// Interactively edit the local person's task-tag set via the toggle-checklist.
///
/// Started with the current tags, or the generic defaults if none. Kept tags
/// retain their styling; new tags get a neutral style. Used by `brain config
/// set tags`.
pub fn run_set_tags(workspace: &WorkspaceContext) -> Result<()> {
    let user_id = workspace.local_user_id().to_owned();
    let mut persona = store::load_persona(workspace, &user_id);
    let initial: Vec<String> = if persona.tag_styles.is_empty() {
        super::tags::default_tag_names()
    } else {
        persona.tag_styles.keys().cloned().collect()
    };
    match super::checklist::choose("Task tags", &initial, super::tags::normalize_tag)? {
        Some(sel) => {
            persona.tag_styles = super::tags::styles_from_names(&sel, &persona.tag_styles);
            store::save_persona(workspace, &user_id, &persona)?;
            crate::skills::resync_skills(workspace);
            let names: Vec<String> = persona.tag_styles.keys().cloned().collect();
            println!("tags: {}", names.join(", "));
        }
        None => println!("tags unchanged"),
    }
    Ok(())
}

/// `brain persona edit` — open the raw JSON store in `$EDITOR`, then re-render
/// skills. Ensures the file exists first so the editor opens a real
/// (keyed-schema) document.
pub fn run_edit(workspace: &WorkspaceContext) -> Result<()> {
    let existing = store::load(workspace);
    store::save(workspace, &existing)?; // materialize the keyed schema if absent
    let path = store::path_in_config_dir(&crate::settings::config_dir(workspace));
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
    crate::skills::resync_skills(workspace);
    Ok(())
}

/// Minimal single-quote shell escaping for the editor path.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Persona {
        Persona {
            name: "Pablo".to_owned(),
            role: "CEO".to_owned(),
            works_for: "Avandar".to_owned(),
            ..Persona::default()
        }
    }

    #[test]
    fn summary_block_emits_stable_keyed_lines() {
        let block = summary_block(&sample());
        // namespaces falls back to the generic defaults when unset.
        assert_eq!(
            block,
            "name: Pablo\nrole: CEO\nworks_for: Avandar\nnamespaces: work, personal"
        );
    }

    #[test]
    fn summary_block_shows_configured_namespaces() {
        let mut p = sample();
        p.namespaces = vec![
            "avandar".to_owned(),
            "personal".to_owned(),
            "pole".to_owned(),
        ];
        assert!(summary_block(&p).ends_with("namespaces: avandar, personal, pole"));
    }

    #[test]
    fn summary_block_shows_unset_for_empty_fields() {
        let block = summary_block(&Persona::default());
        assert_eq!(
            block,
            "name: (unset)\nrole: (unset)\nworks_for: (unset)\nnamespaces: work, personal"
        );
    }

    #[test]
    fn a_persona_block_names_its_owner_and_marks_the_local_person() {
        assert!(
            persona_block("pablo", &sample(), true).starts_with("user: pablo (this machine)\n")
        );
        assert!(persona_block("sam", &sample(), false).starts_with("user: sam\nname: Pablo"));
    }

    #[test]
    fn the_roster_block_covers_every_member_including_unpersonalized_ones() {
        let personas = Personas::parse(
            r#"{"schema_version": 2, "personas": {"pablo": {"name": "Pablo", "role": "CEO"}}}"#,
            "pablo",
        );

        let block = roster_block(&personas, &["pablo", "sam"], "pablo");

        assert!(block.contains("user: pablo (this machine)"), "{block}");
        assert!(block.contains("role: CEO"), "{block}");
        // Sam has no entry yet, but a skill must still learn they exist.
        assert!(block.contains("user: sam"), "{block}");
        assert!(block.contains("name: (unset)"), "{block}");
        assert_eq!(block.matches("user: ").count(), 2, "{block}");
    }

    #[test]
    fn the_roster_block_always_includes_the_local_person() {
        // A workspace with no portable user store still reports its own reader.
        let block = roster_block(&Personas::default(), &[], "pablo");

        assert!(block.starts_with("user: pablo (this machine)"), "{block}");
    }

    #[test]
    fn a_persona_stored_for_someone_off_the_roster_is_still_listed() {
        // Removing a member should not make their stored persona invisible.
        let personas = Personas::parse(
            r#"{"schema_version": 2, "personas": {"ghost": {"role": "founder"}}}"#,
            "pablo",
        );

        let block = roster_block(&personas, &["pablo"], "pablo");

        assert!(block.contains("user: ghost"), "{block}");
    }

    #[test]
    fn get_field_reads_known_fields_and_none_for_empty_or_unknown() {
        let p = sample();
        assert_eq!(get_field(&p, "role").as_deref(), Some("CEO"));
        assert_eq!(get_field(&Persona::default(), "role"), None);
        assert_eq!(get_field(&p, "bogus"), None);
    }

    #[test]
    fn set_field_updates_known_fields() {
        let mut p = Persona::default();
        set_field(&mut p, "role", "student").unwrap();
        set_field(&mut p, "works_for", "myself").unwrap();
        assert_eq!(p.role, "student");
        assert_eq!(p.works_for, "myself");
    }

    #[test]
    fn set_field_rejects_unknown_field() {
        let mut p = Persona::default();
        assert!(set_field(&mut p, "nope", "x").is_err());
    }

    #[test]
    fn set_field_points_tag_styles_at_edit() {
        let mut p = Persona::default();
        let err = set_field(&mut p, "tag_styles", "{}").unwrap_err();
        assert!(err.to_string().contains("edit"));
    }

    #[test]
    fn an_unknown_user_is_rejected_with_the_members_the_workspace_knows() {
        let roster = ["pablo".to_owned(), "sam".to_owned()];

        assert!(validate_user(&roster, "pablo").is_ok());
        let error = validate_user(&roster, "ghost").unwrap_err().to_string();
        assert!(error.contains("unknown user `ghost`"), "{error}");
        assert!(error.contains("pablo, sam"), "{error}");
    }

    #[test]
    fn a_workspace_with_no_portable_users_accepts_any_id() {
        // Legacy, pre-migration workspaces must still be personalizable.
        assert!(validate_user(&[], "pablo").is_ok());
    }
}
