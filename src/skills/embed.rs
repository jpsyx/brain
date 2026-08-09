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
        // It delegates placement to second-brain rather than re-deriving PARA.
        assert!(text.contains("second-brain"), "delegates to /second-brain");
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
        // Namespaces are runtime config (onboarding checklist), not hardcoded,
        // and a shared workspace has one persona per member.
        assert!(
            text.contains("brain persona show"),
            "reads namespaces/identity at runtime"
        );
        assert!(
            text.contains("brain persona list"),
            "reads every member's persona, not just one"
        );
        for hook in [
            "second-brain:company-context",
            "second-brain:reference-manager",
        ] {
            assert!(
                text.contains(&format!("brain:ext {hook}")),
                "declares the `{hook}` extension hook"
            );
        }
        // Cloud-sync (brain sync) commands, distinct from the lookup-CSV
        // `/second-brain reindex` above. (The docs/ writeup for the broader
        // cloud-sync feature lands in a separate docs task.)
        assert!(
            text.contains("/second-brain cloud-sync"),
            "documents /second-brain cloud-sync"
        );
        assert!(
            text.contains("/second-brain resolve-conflicts"),
            "documents /second-brain resolve-conflicts"
        );
        assert!(
            text.contains("brain sync conflicts --json"),
            "documents the conflicts --json invocation"
        );
        assert!(
            text.contains("brain sync resolve"),
            "documents brain sync resolve"
        );
        assert!(
            text.contains("different operation from"),
            "documents the cloud-sync vs lookup-sync distinction"
        );
        assert!(
            text.contains("\"sync my brain\"") && text.contains("\"pull latest brain changes\""),
            "user-directed sync phrases route to the cloud sync workflow"
        );
        assert!(
            !text.contains("a bare \"sync my brain\" with no signal"),
            "the direct user phrase must not trigger an unnecessary clarification"
        );
        // The local lookup/metadata rebuild is named "reindex", so "sync"
        // unambiguously means cloud sync everywhere. It is the native
        // `brain reindex` command — the old `/second-brain sync` name and the
        // never-shipped `sync.py` / `reindex.py` scripts are gone.
        assert!(
            text.contains("/second-brain reindex"),
            "documents the renamed /second-brain reindex operation"
        );
        assert!(
            text.contains("brain reindex"),
            "invokes the native brain reindex command"
        );
        assert!(
            !text.contains("reindex.py"),
            "no stale reindex.py script references"
        );
        assert!(
            !text.contains("sync.py"),
            "no stale sync.py references remain"
        );
        assert!(
            !text.contains("/second-brain sync"),
            "the old /second-brain sync (lookup-rebuild) name is gone"
        );
        // A bare "do a sync" now routes straight to cloud sync.
        assert!(
            text.contains("\"do a sync\""),
            "a bare 'do a sync' request routes to the cloud sync workflow"
        );
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
        // Ships its deterministic CLI and declares the Notion-fallback hook.
        assert!(
            contacts
                .files
                .iter()
                .any(|f| f.rel_path.as_path() == Path::new("scripts/contacts.py")),
            "bundles scripts/contacts.py"
        );
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
        for p in ["SKILL.md", "scripts/add_task.py", "references/schema.md"] {
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
        // Ships its generic references + its script suite.
        for r in [
            "references/schema.md",
            "references/commands.md",
            "scripts/add_task.py",
        ] {
            assert!(
                todo.files
                    .iter()
                    .any(|f| f.rel_path.as_path() == Path::new(r)),
                "bundles {r}"
            );
        }
    }
}
