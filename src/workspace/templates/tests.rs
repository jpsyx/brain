use super::*;

fn workspace_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("temporary workspace root")
}

#[test]
fn an_empty_workspace_receives_the_agent_instructions() {
    let root = workspace_root();

    seed_documents(root.path()).unwrap();

    assert_eq!(
        std::fs::read_to_string(root.path().join("AGENTS.md")).unwrap(),
        AGENTS
    );
}

/// A second orientation document is a second copy of the same facts, and the
/// copy no frontend reads is the one that goes stale. `AGENTS.md` describes
/// the layout itself; nothing seeds a `README.md` over it.
#[test]
fn no_second_orientation_document_is_seeded() {
    let root = workspace_root();

    seed_documents(root.path()).unwrap();

    assert!(!root.path().join("README.md").exists());
}

/// These become the user's documents the moment they exist: an edited AGENTS.md
/// must survive every later launch, exactly like the portable manifest.
#[test]
fn existing_documents_are_never_overwritten() {
    let root = workspace_root();
    std::fs::write(root.path().join("AGENTS.md"), b"my own rules\n").unwrap();

    seed_documents(root.path()).unwrap();

    assert_eq!(
        std::fs::read(root.path().join("AGENTS.md")).unwrap(),
        b"my own rules\n"
    );
}

/// A workspace that already had one keeps it. Dropping the template is Brain's
/// decision about what it *writes*, never a licence to delete a document that
/// became the user's the moment it existed.
#[test]
fn an_existing_readme_is_left_alone() {
    let root = workspace_root();
    std::fs::write(root.path().join("README.md"), b"my own readme\n").unwrap();

    seed_documents(root.path()).unwrap();

    assert_eq!(
        std::fs::read(root.path().join("README.md")).unwrap(),
        b"my own readme\n"
    );
}

#[test]
fn seeding_is_idempotent() {
    let root = workspace_root();

    seed_documents(root.path()).unwrap();
    let first = std::fs::read(root.path().join("AGENTS.md")).unwrap();
    seed_documents(root.path()).unwrap();

    assert_eq!(std::fs::read(root.path().join("AGENTS.md")).unwrap(), first);
}

/// The templates ship to every brain user, so they must describe the product,
/// not the machine they were derived from. That instance hardcoded `~/brain`,
/// linked into a private global-skills directory, and named skills that are one
/// user's plugins rather than anything brain bundles.
#[test]
fn the_templates_carry_nothing_instance_specific() {
    for (name, body) in [("AGENTS.md", AGENTS)] {
        let lowercase = body.to_lowercase();
        for forbidden in [
            "~/brain",
            "/users/",
            "global-skills",
            "pablo",
            "avandar",
            "hubspot",
            "linear-sync",
            "zotero",
            "ict4d",
        ] {
            assert!(
                !lowercase.contains(forbidden),
                "{name} must stay generic; found {forbidden}"
            );
        }
    }
}

/// A template naming a skill brain does not ship sends the reader after
/// something that will never exist for them.
#[test]
fn every_skill_the_templates_name_is_one_brain_bundles() {
    const BUNDLED: [&str; 6] = [
        "second-brain",
        "todo",
        "triage",
        "contacts",
        "brain-knowledge-capture",
        "article-summarizer",
    ];
    // The tool's own name reads exactly like a skill name and is unavoidable.
    const NOT_A_SKILL: &str = "brain";

    // Only passages that actually talk about skills, so an example project slug
    // elsewhere (`launch-team-handbook`) is not mistaken for one.
    for body in [AGENTS] {
        for paragraph in body.split("\n\n") {
            if !paragraph.contains("skill") {
                continue;
            }
            for quoted in paragraph.split('`').skip(1).step_by(2) {
                let candidate = quoted.trim().trim_matches('*');
                if candidate.is_empty()
                    || candidate.contains([' ', '/', '<', '.', '_'])
                    || candidate == NOT_A_SKILL
                {
                    continue;
                }
                assert!(
                    BUNDLED.contains(&candidate),
                    "`{candidate}` is named where skills are discussed, but brain bundles no such skill"
                );
            }
        }
    }
}

/// With no companion document, the agent instructions have to answer "what is
/// this directory?" on their own, and must not send the reader to a file that
/// no longer exists.
#[test]
fn the_agent_instructions_describe_the_layout_themselves() {
    for directory in [
        "`projects/`",
        "`areas/`",
        "`resources/`",
        "`archive/`",
        "`capture/`",
        "`tasks/`",
    ] {
        assert!(AGENTS.contains(directory), "AGENTS.md never names {directory}");
    }
    assert!(!AGENTS.contains("README.md"), "AGENTS.md links a document brain no longer seeds");
}
