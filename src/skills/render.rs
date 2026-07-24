//! Rendering a skill into the files to install.
//!
//! The base skill's `SKILL.md` is injected with the user's extension (if any)
//! via `extension::apply`, producing a **new built copy** — the repo/plugin
//! source is never mutated. All other files pass through unchanged.

use std::path::{Path, PathBuf};

use super::extension::{self, Extension};
use super::model::Skill;

/// A file to write into the built skill dir.
pub struct RenderedFile {
    pub rel_path: PathBuf,
    pub contents: Vec<u8>,
}

/// Render `skill` to its installable files, injecting `ext` into `SKILL.md`.
#[must_use]
pub fn render(skill: &Skill, ext: Option<&Extension>) -> Vec<RenderedFile> {
    skill
        .files
        .iter()
        .map(|f| {
            let contents = if is_skill_md(&f.rel_path) {
                ext.map_or_else(
                    || f.contents.clone(),
                    |e| extension::apply(&String::from_utf8_lossy(&f.contents), e).into_bytes(),
                )
            } else {
                f.contents.clone()
            };
            RenderedFile {
                rel_path: f.rel_path.clone(),
                contents,
            }
        })
        .collect()
}

fn is_skill_md(rel: &Path) -> bool {
    rel == Path::new("SKILL.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::model::SkillFile;

    fn skill_md(body: &str) -> Skill {
        Skill {
            name: "t".to_owned(),
            files: vec![SkillFile {
                rel_path: PathBuf::from("SKILL.md"),
                contents: body.as_bytes().to_vec(),
            }],
        }
    }

    #[test]
    fn render_without_extension_is_passthrough() {
        let s = skill_md("# hi\nbody\n");
        let out = render(&s, None);
        assert_eq!(out[0].contents, b"# hi\nbody\n");
    }

    #[test]
    fn render_with_extension_injects_into_skill_md() {
        let s = skill_md("# T\n<!-- brain:ext t:start -->\nsteps\n");
        let mut ext = Extension::default();
        ext.hooks.insert("t:start".to_owned(), "FIRST".to_owned());
        let out = render(&s, Some(&ext));
        let text = String::from_utf8(out[0].contents.clone()).unwrap();
        assert!(text.contains("FIRST"));
        assert!(!text.contains("brain:ext"));
    }
}
