use super::*;

fn workspace_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("temporary workspace root")
}

#[test]
fn an_empty_workspace_receives_the_agent_instructions() {
    let root = workspace_root();

    seed_documents(root.path()).unwrap();

    assert!(
        std::fs::read_to_string(root.path().join("AGENTS.md"))
            .unwrap()
            .contains(&render_contract())
    );
}

/// One orientation document avoids duplicate, potentially stale guidance.
/// AGENTS.md describes the layout without seeding a second README.
#[test]
fn no_second_orientation_document_is_seeded() {
    let root = workspace_root();

    seed_documents(root.path()).unwrap();

    assert!(!root.path().join("README.md").exists());
}

/// Initialization leaves existing documents alone. Managed package
/// reconciliation, rather than seeding, owns subsequent region updates.
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
    for (name, body) in [("AGENTS.md", render_contract())] {
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

/// New route names can appear on an older installation only with an explicit
/// supported fallback; they must not send the reader to an unavailable workflow.
#[test]
fn new_skill_routes_include_their_legacy_fallbacks() {
    let contract = render_contract();
    for (canonical, fallback) in [
        ("projects", "`second-brain`"),
        ("process-capture", "`second-brain`"),
        ("organize", "`second-brain`"),
        ("brain-sync", "`second-brain`"),
        ("agenda", "`todo`"),
        ("brain-skills", "Authoring instructions below"),
    ] {
        let row = contract
            .lines()
            .find(|line| line.starts_with(&format!("| `{canonical}` |")))
            .expect("canonical route");
        assert!(
            row.contains(fallback),
            "missing legacy route for {canonical}"
        );
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
        assert!(
            render_contract().contains(directory),
            "AGENTS.md never names {directory}"
        );
    }
    assert!(
        !render_contract().contains("README.md"),
        "AGENTS.md links a document brain no longer seeds"
    );
}

#[test]
fn seeded_instructions_are_managed_and_expand_the_response_policy() {
    let root = workspace_root();
    seed_documents(root.path()).unwrap();
    let document = std::fs::read_to_string(root.path().join("AGENTS.md")).unwrap();
    assert!(document.starts_with("<!-- brain:managed-workspace:start schema=1 -->\n"));
    assert!(document.ends_with("<!-- brain:managed-workspace:end -->\n"));
    assert!(document.contains(&crate::response_policy::render_workspace_reference()));
    assert!(!document.contains("{{"));
}
