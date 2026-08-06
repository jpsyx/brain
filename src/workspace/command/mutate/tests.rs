use std::path::{Path, PathBuf};

use super::{Mutation, MutationInput, decide_mutation};
use crate::workspace::WorkspaceName;

fn name(value: &str) -> WorkspaceName {
    WorkspaceName::parse(value).expect("valid fixture name")
}

#[test]
fn create_decision_expands_normalizes_and_derives_a_missing_name() {
    let decision = decide_mutation(
        MutationInput::Create {
            name: None,
            root: Path::new("~/brains/../Family"),
        },
        Path::new("/home/tester"),
        Path::new("/work"),
    )
    .expect("valid create decision");

    assert_eq!(
        decision,
        Mutation::Create {
            canonical_name: name("family"),
            root: PathBuf::from("/home/tester/Family"),
        }
    );
}

#[test]
fn attach_decision_normalizes_relative_root_and_derives_name() {
    let decision = decide_mutation(
        MutationInput::Attach {
            root: Path::new("shared/../Family"),
        },
        Path::new("/home/tester"),
        Path::new("/workspaces"),
    )
    .expect("valid attach decision");

    assert_eq!(
        decision,
        Mutation::Attach {
            canonical_name: name("family"),
            root: PathBuf::from("/workspaces/Family"),
        }
    );
}

#[test]
fn rename_alias_and_default_decisions_validate_new_names_only() {
    let cases = [
        (
            MutationInput::Rename {
                selector: "Fam",
                new_name: "Shared_Home",
            },
            Mutation::Rename {
                selector: "Fam".to_owned(),
                new_name: name("shared_home"),
            },
        ),
        (
            MutationInput::AddAlias {
                selector: "family",
                alias: "Fam",
            },
            Mutation::AddAlias {
                selector: "family".to_owned(),
                alias: name("fam"),
            },
        ),
        (
            MutationInput::RemoveAlias {
                selector: "family",
                alias: "Fam",
            },
            Mutation::RemoveAlias {
                selector: "family".to_owned(),
                alias: name("fam"),
            },
        ),
        (
            MutationInput::SetDefault { selector: "Fam" },
            Mutation::SetDefault {
                selector: "Fam".to_owned(),
            },
        ),
    ];

    for (input, expected) in cases {
        assert_eq!(
            decide_mutation(input, Path::new("/home/tester"), Path::new("/work"))
                .expect("valid mutation decision"),
            expected
        );
    }
}

#[test]
fn remove_decision_describes_only_a_registry_selector() {
    let decision = decide_mutation(
        MutationInput::Remove { selector: "Fam" },
        Path::new("/home/tester"),
        Path::new("/work"),
    )
    .expect("valid remove decision");

    assert_eq!(
        decision,
        Mutation::Remove {
            selector: "Fam".to_owned(),
        }
    );
}
