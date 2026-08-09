//! Locating and reading/writing the personalization store.
//!
//! Personas are just another brain config, so they live at
//! `personalization.json` in the brain config dir (`<brain-root>/.config/`)
//! alongside `config.json` — **inside** the brain root, so they travel with the
//! brain and every machine on the workspace sees the same people. A missing or
//! broken file reads as the default value; it never blocks startup.
//!
//! The file is keyed by portable user ID (see [`super::personas`]). Reads that
//! concern one person go through [`load_persona`] / [`local_persona`]; writes
//! go through [`save_persona`], which never disturbs another member's entry.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::persona::Persona;
use super::personas::Personas;

/// The store path within a config dir: `<config-dir>/personalization.json`.
/// Pure (no IO) so it is testable without touching the environment.
#[must_use]
pub fn path_in_config_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("personalization.json")
}

/// Resolve the store path against the brain config dir (`~/.config/brain`).
fn store_path(workspace: &crate::workspace::WorkspaceContext) -> PathBuf {
    path_in_config_dir(&crate::settings::config_dir(workspace))
}

/// Read every persona. Any failure (missing file, unreadable, invalid JSON)
/// yields no personas.
///
/// A legacy single-persona store migrates onto this machine's local user, who
/// is the only person able to claim an unowned record.
#[must_use]
pub fn load(workspace: &crate::workspace::WorkspaceContext) -> Personas {
    std::fs::read_to_string(store_path(workspace))
        .map(|text| Personas::parse(&text, workspace.local_user_id()))
        .unwrap_or_default()
}

/// One user's persona, empty when they have no entry.
#[must_use]
pub fn load_persona(workspace: &crate::workspace::WorkspaceContext, user_id: &str) -> Persona {
    load(workspace).persona_of(user_id)
}

/// This machine's local person's persona.
#[must_use]
pub fn local_persona(workspace: &crate::workspace::WorkspaceContext) -> Persona {
    load_persona(workspace, workspace.local_user_id())
}

/// Persist every persona, creating the config dir if needed.
pub fn save(workspace: &crate::workspace::WorkspaceContext, personas: &Personas) -> Result<()> {
    let path = store_path(workspace);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, format!("{}\n", personas.to_json()?))?;
    Ok(())
}

/// Replace one user's persona, preserving every other member's entry.
pub fn save_persona(
    workspace: &crate::workspace::WorkspaceContext,
    user_id: &str,
    persona: &Persona,
) -> Result<()> {
    let mut personas = load(workspace);
    personas.set(user_id, persona.clone());
    save(workspace, &personas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lives_beside_config_json_in_the_brain_config_dir() {
        assert_eq!(
            path_in_config_dir(Path::new("/Users/x/.config/brain")),
            PathBuf::from("/Users/x/.config/brain/personalization.json")
        );
    }

    fn workspace_at(home: &Path, local_user_id: &str) -> crate::workspace::WorkspaceContext {
        let root = home.join("brain");
        std::fs::create_dir_all(root.join(".config")).unwrap();
        crate::workspace::WorkspaceContext::new(
            home,
            crate::workspace::WorkspaceId::new(),
            crate::workspace::WorkspaceName::parse("brain").unwrap(),
            &root,
            local_user_id,
            home,
        )
        .unwrap()
    }

    #[test]
    fn a_legacy_single_persona_store_loads_under_this_machines_local_user() {
        let home = tempfile::tempdir().unwrap();
        let workspace = workspace_at(home.path(), "pablo");
        std::fs::write(
            store_path(&workspace),
            r#"{"name": "Pablo", "role": "CEO", "works_for": "Avandar"}"#,
        )
        .unwrap();

        let personas = load(&workspace);

        assert_eq!(personas.ids(), ["pablo"]);
        assert_eq!(personas.persona_of("pablo").role, "CEO");
    }

    #[test]
    fn saving_rewrites_the_legacy_store_in_the_keyed_schema() {
        let home = tempfile::tempdir().unwrap();
        let workspace = workspace_at(home.path(), "pablo");
        std::fs::write(store_path(&workspace), r#"{"role": "CEO"}"#).unwrap();

        let personas = load(&workspace);
        save(&workspace, &personas).unwrap();

        let written = std::fs::read_to_string(store_path(&workspace)).unwrap();
        assert!(written.contains("\"personas\""), "{written}");
        assert_eq!(load(&workspace).persona_of("pablo").role, "CEO");
    }

    #[test]
    fn one_users_persona_reads_and_writes_without_touching_another() {
        let home = tempfile::tempdir().unwrap();
        let workspace = workspace_at(home.path(), "pablo");
        save_persona(
            &workspace,
            "sam",
            &Persona {
                role: "designer".to_owned(),
                ..Persona::default()
            },
        )
        .unwrap();
        save_persona(
            &workspace,
            "pablo",
            &Persona {
                role: "CEO".to_owned(),
                ..Persona::default()
            },
        )
        .unwrap();

        assert_eq!(load_persona(&workspace, "sam").role, "designer");
        assert_eq!(local_persona(&workspace).role, "CEO");
    }

    #[test]
    fn a_missing_store_reads_as_an_empty_persona_for_anyone() {
        let home = tempfile::tempdir().unwrap();
        let workspace = workspace_at(home.path(), "pablo");

        assert!(load(&workspace).is_empty());
        assert!(load_persona(&workspace, "nobody").is_empty());
    }

    #[test]
    fn resolved_store_path_is_under_the_brain_config_dir() {
        let workspace = crate::workspace::WorkspaceContext::new(
            Path::new("/home/tester"),
            crate::workspace::WorkspaceId::new(),
            crate::workspace::WorkspaceName::parse("brain").unwrap(),
            Path::new("/home/tester/brain"),
            "tester",
            Path::new("/home/tester"),
        )
        .unwrap();
        let p = store_path(&workspace);
        assert!(p.ends_with(".config/personalization.json"));
        assert_eq!(
            p.parent(),
            Some(crate::settings::config_dir(&workspace).as_path())
        );
    }
}
