use super::{catalog, navigation};
use brain::skills::{install, layout::Layout};
use std::fs;

#[test]
fn complete_rendered_navigation_survives_every_supported_customization_shape() {
    let legacy = "[todo:anchors]\nAn explicit workspace anchor.\n[todo:agenda-after-build]\nVerify contributed output.\n";
    let canonical = "[agenda:anchors]\nAn explicit workspace anchor.\n[agenda:after-build]\nVerify contributed output.\n";
    for variant in ["none", "legacy", "canonical", "mixed", "plugin"] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let extensions = root.join("extensions");
        let plugins = root.join("plugins");
        fs::create_dir(&extensions).unwrap();
        fs::create_dir(&plugins).unwrap();
        if matches!(variant, "legacy" | "mixed") {
            fs::write(extensions.join("todo.md"), legacy).unwrap();
            fs::write(extensions.join("second-brain.md"), "[second-brain:company-context]\nShared context.\n[second-brain:reference-manager]\nResource context.\n").unwrap();
        }
        if matches!(variant, "canonical" | "mixed") {
            fs::write(extensions.join("agenda.md"), canonical).unwrap();
            fs::write(
                extensions.join("workspace.md"),
                "[workspace:context]\nShared context.\n",
            )
            .unwrap();
            fs::write(
                extensions.join("organize.md"),
                "[organize:reference-manager]\nResource context.\n",
            )
            .unwrap();
        }
        if variant == "plugin" {
            let plugin = plugins.join("review-helper");
            fs::create_dir(&plugin).unwrap();
            fs::write(plugin.join("SKILL.md"), "---\nname: review-helper\ndescription: Synthetic navigation fixture\n---\n# Review helper\n\n[script](helper.py) [asset](asset.bin) [old agenda](../todo/SKILL.md#agenda-generation--phased-procedure) [project](../second-brain/SKILL.md#commands). Use /second-brain cloud-sync and /todo agenda.\n").unwrap();
            fs::write(plugin.join("helper.py"), "print('synthetic')\n").unwrap();
            fs::write(plugin.join("asset.bin"), [0, 255]).unwrap();
        }
        install::sync(
            &Layout::under_root(root),
            &install::Sources {
                extensions_dir: Some(extensions),
                plugins_dir: Some(plugins),
            },
        )
        .unwrap();
        let built = root.join(".agents/skills");
        let files: catalog::Files = walkdir::WalkDir::new(&built)
            .into_iter()
            .map(Result::unwrap)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| {
                (
                    entry
                        .path()
                        .strip_prefix(&built)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect();
        navigation::validate(&files);
        if variant != "none" && variant != "plugin" {
            let docs = catalog::markdown_documents(&files);
            assert_eq!(
                docs["agenda/SKILL.md"]
                    .matches("An explicit workspace anchor.")
                    .count(),
                1,
                "{variant}"
            );
        }
    }
}
