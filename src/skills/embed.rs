//! The bundled skills, embedded into the binary from the repo's `skills/` dir
//! (SKILL.md + any scripts), so a public cloner needs no repo checkout.

use include_dir::{Dir, File, include_dir};

use super::model::{Skill, SkillFile};

static SKILLS: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");

/// Every bundled skill (each top-level dir under `skills/`).
#[must_use]
pub fn bundled_skills() -> Vec<Skill> {
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
                    Some(SkillFile {
                        rel_path: rel,
                        contents: f.contents().to_vec(),
                    })
                })
                .collect();
            Some(Skill { name, files })
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
    use std::path::Path;

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
            .find(|f| f.rel_path.as_path() == Path::new("SKILL.md"))
            .expect("SKILL.md should be embedded");
        let text = String::from_utf8_lossy(&skill_md.contents);
        assert!(text.contains("article-summarizer"));
    }

    fn skill_md_text(skill: &Skill) -> String {
        let f = skill
            .files
            .iter()
            .find(|f| f.rel_path.as_path() == Path::new("SKILL.md"))
            .expect("every bundled skill has a SKILL.md");
        String::from_utf8_lossy(&f.contents).into_owned()
    }

    /// The repo is public: no bundled skill may carry personal identity, private
    /// tool paths, or private URLs. Personal behavior lives in the user's
    /// extensions/plugins (`<brain>/.config/...`), never in the bundle.
    #[test]
    fn bundled_skills_carry_no_personal_data() {
        // Case-insensitive substrings that must never appear in a bundled skill.
        const FORBIDDEN: &[&str] = &[
            "pablo@avandarlabs.com",
            "pablowritescode@gmail.com",
            "avandar",
            "busy ceo",
            "notion.so/pablosarmiento",
            "25a190d5dfe8809291afdd1acec62450", // Pablo's Notion In-Basket block id
            "~/global-skills/",
            "~/scripts/",
            "~/downloads",
        ];
        for skill in bundled_skills() {
            for f in &skill.files {
                let text = String::from_utf8_lossy(&f.contents).to_lowercase();
                for needle in FORBIDDEN {
                    assert!(
                        !text.contains(needle),
                        "bundled skill `{}` file `{}` contains personal token `{needle}`",
                        skill.name,
                        f.rel_path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn bundles_the_generic_triage_skill() {
        let skills = bundled_skills();
        let triage = skills
            .iter()
            .find(|s| s.name == "triage")
            .expect("triage should be embedded");
        let text = skill_md_text(triage);
        assert!(text.contains("# triage"), "has the triage heading");
        // Its personalization-point markers, filled by the user's triage extension.
        for hook in [
            "triage:daily-open",
            "triage:daily-linear",
            "triage:weekly-inboxes",
            "triage:weekly-linear",
        ] {
            assert!(
                text.contains(&format!("brain:ext {hook}")),
                "declares the `{hook}` extension hook"
            );
        }
        // The historical heuristics reference file ships alongside it.
        assert!(
            triage
                .files
                .iter()
                .any(|f| f.rel_path.as_path() == Path::new("references/heuristics.md")),
            "bundles references/heuristics.md"
        );
    }

    #[test]
    fn bundles_the_generic_brain_knowledge_capture_skill() {
        let skills = bundled_skills();
        let capture = skills
            .iter()
            .find(|s| s.name == "brain-knowledge-capture")
            .expect("brain-knowledge-capture should be embedded");
        let text = skill_md_text(capture);
        assert!(text.contains("# brain-knowledge-capture"), "has the heading");
        // It delegates placement to second-brain rather than re-deriving PARA.
        assert!(text.contains("second-brain"), "delegates to /second-brain");
    }
}
