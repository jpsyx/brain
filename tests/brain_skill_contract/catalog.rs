use std::collections::BTreeMap;

pub type Documents = BTreeMap<String, String>;
pub type Files = BTreeMap<String, Vec<u8>>;

pub fn files(rendered: bool) -> Files {
    let mut inventory = Files::new();
    for skill in brain::skills::embed::bundled_skills() {
        let files: Vec<_> = if rendered {
            brain::skills::render::render(&skill, None)
                .into_iter()
                .map(|file| (file.rel_path, file.contents))
                .collect()
        } else {
            skill
                .files
                .into_iter()
                .map(|file| (file.rel_path, file.contents))
                .collect()
        };
        for (path, contents) in files {
            inventory.insert(format!("{}/{}", skill.name, path.display()), contents);
        }
    }
    inventory
}

pub fn documents(rendered: bool) -> Documents {
    markdown_documents(&files(rendered))
}

pub fn markdown_documents(files: &Files) -> Documents {
    files
        .iter()
        .filter(|(path, _)| {
            std::path::Path::new(path)
                .extension()
                .is_some_and(|extension| extension == "md")
        })
        .map(|(path, contents)| (path.clone(), String::from_utf8(contents.clone()).unwrap()))
        .collect()
}

#[test]
fn focused_workflows_are_available_in_source_and_rendered_catalogs() {
    for rendered in [false, true] {
        let documents = documents(rendered);
        for name in ["projects", "process-capture", "organize", "brain-sync"] {
            let path = format!("{name}/SKILL.md");
            let body = documents
                .get(&path)
                .unwrap_or_else(|| panic!("missing {path}, rendered={rendered}"));
            assert!(body.starts_with(&format!("---\nname: {name}\n")));
            assert!(
                !super::markdown::links(body).is_empty(),
                "{path} must route to its real collaborators/references"
            );
        }
    }
}
