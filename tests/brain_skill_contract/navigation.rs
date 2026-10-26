use super::{catalog, markdown};
use std::path::{Component, Path};

fn resolve(from: &str, target: &str) -> String {
    let mut parts = Vec::new();
    let path = Path::new(from).parent().unwrap().join(target);
    for component in path.components() {
        match component {
            Component::ParentDir => {
                parts.pop().expect("link escapes the skill catalog");
            }
            Component::Normal(part) => parts.push(part.to_str().unwrap()),
            Component::CurDir => (),
            _ => panic!("relative catalog target expected"),
        }
    }
    parts.join("/")
}

fn validate_link(documents: &catalog::Documents, from: &str, link: &str) {
    let (path, anchor) = link
        .split_once('#')
        .map_or((link, None), |(path, anchor)| (path, Some(anchor)));
    let target = if path.is_empty() {
        from.to_owned()
    } else {
        resolve(from, path)
    };
    let document = documents
        .get(&target)
        .unwrap_or_else(|| panic!("{from}: missing target {link} -> {target}"));
    if let Some(anchor) = anchor {
        assert!(
            markdown::anchors(document).contains(anchor),
            "{from}: missing anchor {link} -> {target}"
        );
    }
}

#[test]
fn every_real_link_and_explicit_skill_route_resolves() {
    for rendered in [false, true] {
        let documents = catalog::documents(rendered);
        for (path, body) in &documents {
            for link in markdown::links(body) {
                validate_link(&documents, path, &link);
            }
            for route in markdown::routes(body) {
                assert!(
                    documents.contains_key(&format!("{route}/SKILL.md")),
                    "{path}: unavailable /{route}"
                );
            }
        }
    }
}

#[test]
fn compatibility_routes_reach_the_focused_owner() {
    for rendered in [false, true] {
        let documents = catalog::documents(rendered);
        let router = &documents["second-brain/SKILL.md"];
        for (alias, owner) in [
            ("/second-brain reindex", "../organize/SKILL.md#reindex"),
            (
                "/second-brain cloud-sync",
                "../brain-sync/SKILL.md#cloud-sync",
            ),
            (
                "/second-brain resolve-conflicts",
                "../brain-sync/SKILL.md#resolve-conflicts",
            ),
            (
                "/second-brain process-capture",
                "../process-capture/SKILL.md",
            ),
            ("/second-brain add-project", "../projects/SKILL.md#create"),
            (
                "/second-brain complete-project",
                "../projects/SKILL.md#complete",
            ),
            (
                "/second-brain archive-project",
                "../projects/SKILL.md#archive",
            ),
            (
                "/second-brain extract-ip",
                "../projects/references/extract-ips.md",
            ),
        ] {
            let row = router
                .lines()
                .find(|line| line.contains(alias))
                .unwrap_or_else(|| panic!("missing route {alias}"));
            assert!(
                markdown::links(row).contains(&owner.to_owned()),
                "{alias} must reach {owner}"
            );
            validate_link(&documents, "second-brain/SKILL.md", owner);
        }
        let old = "todo/references/task-project-link.md";
        let canonical = "../../projects/references/task-project-link.md";
        assert!(markdown::links(&documents[old]).contains(&canonical.to_owned()));
        validate_link(&documents, old, canonical);
    }
}
