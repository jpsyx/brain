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

fn validate_link(files: &catalog::Files, from: &str, link: &str) {
    let (path, anchor) = link
        .split_once('#')
        .map_or((link, None), |(path, anchor)| (path, Some(anchor)));
    let target = if path.is_empty() {
        from.to_owned()
    } else {
        resolve(from, path)
    };
    let document = files
        .get(&target)
        .unwrap_or_else(|| panic!("{from}: missing target {link} -> {target}"));
    if let Some(anchor) = anchor
        && Path::new(&target)
            .extension()
            .is_some_and(|extension| extension == "md")
    {
        assert!(
            markdown::anchors(std::str::from_utf8(document).unwrap()).contains(anchor),
            "{from}: missing anchor {link} -> {target}"
        );
    }
}

pub fn validate(files: &catalog::Files) {
    for (path, body) in catalog::markdown_documents(files) {
        for link in markdown::links(&body) {
            validate_link(files, &path, &link);
        }
        for route in markdown::routes(&body) {
            assert!(
                files.contains_key(&format!("{route}/SKILL.md")),
                "{path}: unavailable /{route}"
            );
        }
    }
}

#[test]
fn every_real_link_and_explicit_skill_route_resolves() {
    for rendered in [false, true] {
        validate(&catalog::files(rendered));
    }
}

#[test]
fn binary_assets_need_existence_while_markdown_fragments_need_real_anchors() {
    let files = catalog::Files::from([
        (
            "custom/SKILL.md".into(),
            b"[asset](asset.bin) [notes](notes.md#real)".to_vec(),
        ),
        ("custom/asset.bin".into(), vec![0, 255]),
        ("custom/notes.md".into(), b"# Real\n".to_vec()),
    ]);
    validate(&files);
    assert!(
        std::panic::catch_unwind(|| validate_link(&files, "custom/SKILL.md", "notes.md#missing"))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| validate_link(&files, "custom/SKILL.md", "absent.bin"))
            .is_err()
    );
}

#[test]
fn compatibility_routes_reach_the_focused_owner() {
    for rendered in [false, true] {
        let files = catalog::files(rendered);
        let documents = catalog::markdown_documents(&files);
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
            validate_link(&files, "second-brain/SKILL.md", owner);
        }
        let old = "todo/references/task-project-link.md";
        let canonical = "../../projects/references/task-project-link.md";
        assert!(markdown::links(&documents[old]).contains(&canonical.to_owned()));
        validate_link(&files, old, canonical);
    }
}
