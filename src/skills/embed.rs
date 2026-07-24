//! The bundled skills, embedded into the binary from the repo's `skills/` dir
//! (SKILL.md + any scripts), so a public cloner needs no repo checkout.

use std::path::PathBuf;

use include_dir::{Dir, File, include_dir};

static SKILLS: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");

/// One embedded skill: its directory name plus every file under it (with paths
/// relative to the skill dir, e.g. `SKILL.md`, `scripts/foo.py`).
pub struct BundledSkill {
    pub name: String,
    pub files: Vec<BundledFile>,
}

pub struct BundledFile {
    pub rel_path: PathBuf,
    pub contents: Vec<u8>,
}

/// Every bundled skill (each top-level dir under `skills/`).
#[must_use]
pub fn bundled_skills() -> Vec<BundledSkill> {
    SKILLS
        .dirs()
        .filter_map(|dir| {
            let name = dir.path().file_name()?.to_string_lossy().into_owned();
            let mut flat = Vec::new();
            collect_files(dir, &mut flat);
            let files = flat
                .into_iter()
                .filter_map(|f| {
                    let rel = f.path().strip_prefix(dir.path()).ok()?.to_path_buf();
                    Some(BundledFile {
                        rel_path: rel,
                        contents: f.contents().to_vec(),
                    })
                })
                .collect();
            Some(BundledSkill { name, files })
        })
        .collect()
}

fn collect_files<'a>(dir: &'a Dir<'a>, out: &mut Vec<&'a File<'a>>) {
    for f in dir.files() {
        out.push(f);
    }
    for sub in dir.dirs() {
        collect_files(sub, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_the_article_summarizer_pilot() {
        let skills = bundled_skills();
        let art = skills
            .iter()
            .find(|s| s.name == "article-summarizer")
            .expect("article-summarizer should be embedded");
        let skill_md = art
            .files
            .iter()
            .find(|f| f.rel_path.as_path() == std::path::Path::new("SKILL.md"))
            .expect("SKILL.md should be embedded");
        let text = String::from_utf8_lossy(&skill_md.contents);
        assert!(text.contains("article-summarizer"));
    }
}
