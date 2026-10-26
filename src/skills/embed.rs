//! The bundled skills, embedded into the binary from the repo's `skills/` dir
//! (SKILL.md + any scripts), so a public cloner needs no repo checkout.

use include_dir::{Dir, File, include_dir};

use super::model::{Skill, SkillFile};

static SKILLS: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");

/// Is this a build artifact rather than skill content? `include_dir!` embeds the
/// `skills/` tree exactly as it sits on the machine doing the build, so anything
/// a contributor's tooling drops in there would otherwise be compiled into the
/// binary and installed into every user's skills dir. Python bytecode is the
/// dangerous case: a `.pyc` records the absolute source path it was compiled
/// from, leaking the builder's filesystem layout into a public artifact.
fn is_build_artifact(rel: &std::path::Path) -> bool {
    if rel
        .components()
        .any(|c| c.as_os_str() == "__pycache__" || c.as_os_str() == ".DS_Store")
    {
        return true;
    }
    matches!(
        rel.extension().and_then(|e| e.to_str()),
        Some("pyc" | "pyo")
    )
}

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
                    if is_build_artifact(&rel) {
                        return None;
                    }
                    Some(SkillFile {
                        rel_path: rel,
                        contents: f.contents().to_vec(),
                    })
                })
                .collect();
            Some(Skill {
                name,
                files,
                bundled: true,
            })
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
            "triage:daily-subagents",
            "triage:daily-linear",
            "triage:daily-merge",
            "triage:weekly-inboxes",
            "triage:weekly-linear",
        ] {
            assert!(
                text.contains(&format!("brain:ext {hook}")),
                "declares the `{hook}` extension hook"
            );
        }
        // Daily triage can run extension-registered sub-agents in parallel, and
        // the final agenda PDF is gated on all of them finishing + merging.
        // Collapse whitespace so line-wrapping/bold in the prose can't break the
        // sentinel match.
        let flat = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        assert!(
            flat.contains("in parallel with the rest of daily triage"),
            "documents launching sub-agents in parallel"
        );
        assert!(
            flat.contains("wait for every registered sub-agent to finish"),
            "gates the final PDF on all sub-agents finishing before merge"
        );
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
    fn bundles_the_brain_skill_authoring_guide() {
        let skills = bundled_skills();
        let guide = skills
            .iter()
            .find(|skill| skill.name == "brain-skills")
            .expect("brain-skills guide should be embedded");
        let text = skill_md_text(guide);
        assert!(text.contains("<brain-root>/.config/plugins/<skill-name>/SKILL.md"));
        assert!(text.contains("brain skills sync"));
    }

    #[test]
    fn bundles_the_generic_brain_knowledge_capture_skill() {
        let skills = bundled_skills();
        let capture = skills
            .iter()
            .find(|s| s.name == "brain-knowledge-capture")
            .expect("brain-knowledge-capture should be embedded");
        let text = skill_md_text(capture);
        assert!(
            text.contains("# brain-knowledge-capture"),
            "has the heading"
        );
        // Placement has one focused owner.
        assert!(
            text.contains("../organize/SKILL.md"),
            "delegates to organize"
        );
    }

    #[test]
    fn bundles_the_generic_second_brain_skill() {
        let skills = bundled_skills();
        let sb = skills
            .iter()
            .find(|s| s.name == "second-brain")
            .expect("second-brain should be embedded");
        let text = skill_md_text(sb);
        assert!(text.contains("# second-brain"), "has the heading");
        // The summary method is delegated, not re-derived.
        assert!(
            text.contains("article-summarizer"),
            "references article-summarizer"
        );
        // The contacts book is its own sibling skill now.
        assert!(text.contains("/contacts"), "points at the /contacts skill");
        for owner in ["projects", "process-capture", "organize", "brain-sync"] {
            assert!(skills.iter().any(|skill| skill.name == owner));
            assert!(text.contains(&format!("../{owner}/")));
        }
        let organize = skills
            .iter()
            .find(|skill| skill.name == "organize")
            .unwrap();
        assert!(skill_md_text(organize).contains("brain:ext organize:reference-manager"));
    }

    #[test]
    fn bundles_the_generic_contacts_skill() {
        let skills = bundled_skills();
        let contacts = skills
            .iter()
            .find(|s| s.name == "contacts")
            .expect("contacts should be embedded");
        let text = skill_md_text(contacts);
        assert!(text.contains("# contacts"), "has the heading");
        // Points at the native command rather than shipping a script.
        assert!(text.contains("brain contacts"), "names the native command");
        assert!(
            text.contains("brain:ext contacts:fallback"),
            "declares the contacts:fallback hook"
        );
    }

    /// Build litter must never reach the bundle. A `.pyc` embeds the absolute
    /// path of the machine that compiled it, so shipping one leaks the
    /// builder's private filesystem layout into a public binary.
    #[test]
    fn build_artifacts_are_not_bundled() {
        for p in [
            "scripts/__pycache__/_csvlib.cpython-314.pyc",
            "references/.DS_Store",
            "scripts/stale.pyo",
        ] {
            assert!(is_build_artifact(Path::new(p)), "{p} is build litter");
        }
        for p in ["SKILL.md", "references/schema.md", "cleanup.sh"] {
            assert!(
                !is_build_artifact(Path::new(p)),
                "{p} is real skill content"
            );
        }
    }

    /// The filter is wired into the bundle, not merely available.
    #[test]
    fn no_bundled_skill_ships_build_litter() {
        for skill in bundled_skills() {
            for f in &skill.files {
                assert!(
                    !is_build_artifact(&f.rel_path),
                    "bundled skill `{}` ships build litter `{}`",
                    skill.name,
                    f.rel_path.display()
                );
            }
        }
    }

    #[test]
    fn bundles_the_generic_todo_skill() {
        let skills = bundled_skills();
        let todo = skills
            .iter()
            .find(|s| s.name == "todo")
            .expect("todo should be embedded");
        let text = skill_md_text(todo);
        assert!(text.contains("# todo"), "has the heading");
        // The personal integrations (Linear, calendar/cutoff) are hooks.
        for hook in ["todo:linear", "todo:calendar"] {
            assert!(
                text.contains(&format!("brain:ext {hook}")),
                "declares the `{hook}` extension hook"
            );
        }
        // Ships its generic references.
        for r in ["references/schema.md", "references/commands.md"] {
            assert!(
                todo.files
                    .iter()
                    .any(|f| f.rel_path.as_path() == Path::new(r)),
                "bundles {r}"
            );
        }
    }

    /// Every deterministic operation is a `brain` subcommand now, so no
    /// bundled skill ships executable code for one. A skill that carried a
    /// script would be a second implementation nobody can test from here.
    #[test]
    fn no_bundled_skill_ships_a_script() {
        for skill in bundled_skills() {
            for file in &skill.files {
                let extension = file
                    .rel_path
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or_default();
                assert!(
                    !matches!(extension, "py" | "sh" | "js" | "rb"),
                    "bundled skill `{}` ships `{}`; make it a brain subcommand instead",
                    skill.name,
                    file.rel_path.display()
                );
            }
        }
    }

    /// The `/<name>` skill invocations a markdown text names, e.g. `/todo add`.
    /// A backtick that closes an inline span (`` `id`/x ``) or a path
    /// (`` `/tmp/x` ``) is not an invocation.
    fn skill_invocations(text: &str) -> Vec<String> {
        let bytes = text.as_bytes();
        text.match_indices("`/")
            .filter(|&(i, _)| i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
            .filter_map(|(i, _)| {
                let rest = &text[i + 2..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
                    .unwrap_or(rest.len());
                let name = &rest[..end];
                let terminator = rest[end..].chars().next();
                (!name.is_empty() && matches!(terminator, Some(' ' | '`'))).then(|| name.to_owned())
            })
            .collect()
    }

    #[test]
    fn skill_invocations_skip_paths_and_closing_backticks() {
        let text = "Run `/todo add` or [`/triage`](x); not `/tmp/x` or `id`/timestamp.";
        assert_eq!(skill_invocations(text), ["todo", "triage"]);
    }

    /// Core skill text knows nothing about extensions, so it may hand work
    /// only to another bundled skill, never to a user's own skill.
    #[test]
    fn bundled_skills_invoke_only_bundled_skills() {
        let skills = bundled_skills();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        for skill in &skills {
            for file in &skill.files {
                let text = String::from_utf8_lossy(&file.contents);
                for invoked in skill_invocations(&text) {
                    assert!(
                        names.contains(&invoked.as_str()),
                        "bundled skill `{}` invokes non-bundled skill `/{invoked}` in `{}`",
                        skill.name,
                        file.rel_path.display()
                    );
                }
            }
        }
    }
}
