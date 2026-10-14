use super::{
    fixture::{AGENDA, fake_renderer, fixture},
    today,
};
use crate::tasks::agenda::{
    RemovalAgendaChange, RemovalOutcome, RemovalStage, sync_removal_targets,
};

fn changes() -> [RemovalAgendaChange; 1] {
    [RemovalAgendaChange {
        removed_id: "T535".into(),
        successor_id: None,
    }]
}

#[test]
fn removal_io_distinguishes_absence_read_failure_and_snapshot_failure() {
    let f = fixture(None);
    assert_eq!(
        sync_removal_targets(&f.targets, &changes(), today()),
        RemovalOutcome::NoAgenda
    );
    std::fs::create_dir(&f.targets.markdown).unwrap();
    assert_eq!(
        sync_removal_targets(&f.targets, &changes(), today()),
        RemovalOutcome::Warning {
            stage: RemovalStage::AgendaRead
        }
    );
    std::fs::remove_dir(&f.targets.markdown).unwrap();
    std::fs::write(&f.targets.markdown, AGENDA).unwrap();
    for csv in ["tasks.csv", "habits.csv"] {
        let path = f.targets.tasks_dir.join(csv);
        let saved = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            sync_removal_targets(&f.targets, &changes(), today()),
            RemovalOutcome::Warning {
                stage: RemovalStage::SnapshotRead
            }
        );
        assert_eq!(
            std::fs::read_to_string(&f.targets.markdown).unwrap(),
            AGENDA
        );
        std::fs::write(path, saved).unwrap();
    }
}

#[test]
fn removal_io_updates_once_then_unchanged_and_does_not_create_printable() {
    let f = fixture(Some(AGENDA));
    assert_eq!(
        sync_removal_targets(&f.targets, &changes(), today()),
        RemovalOutcome::Updated { pdf: false }
    );
    let out = std::fs::read_to_string(&f.targets.markdown).unwrap();
    assert!(!out.contains("**T535**"));
    assert_eq!(
        sync_removal_targets(&f.targets, &changes(), today()),
        RemovalOutcome::Unchanged
    );
    assert!(!f.targets.pdf.exists());
}

#[test]
fn removal_pdf_missing_renderer_warns_after_markdown_write_and_preserves_old_printable() {
    let f = fixture(Some(AGENDA));
    std::fs::write(&f.targets.pdf, b"old printable").unwrap();
    assert_eq!(
        sync_removal_targets(&f.targets, &changes(), today()),
        RemovalOutcome::Warning {
            stage: RemovalStage::PdfRender
        }
    );
    assert_eq!(std::fs::read(&f.targets.pdf).unwrap(), b"old printable");
    assert!(
        !std::fs::read_to_string(&f.targets.markdown)
            .unwrap()
            .contains("**T535**")
    );
}

#[test]
fn removal_pdf_failure_or_invalid_output_keeps_old_printable_and_cleans_staging() {
    use std::os::unix::fs::PermissionsExt;
    for script in ["exit 7", "exit 0", ": > \"$3\"", "ln -s \"$1\" \"$3\""] {
        let mut f = fixture(Some(AGENDA));
        let parent = f.targets.pdf.parent().unwrap().to_owned();
        let renderer = parent.join("renderer");
        std::fs::write(&renderer, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&renderer, std::fs::Permissions::from_mode(0o755)).unwrap();
        f.targets.renderer = Some(renderer);
        std::fs::write(&f.targets.pdf, b"old printable").unwrap();
        let before: std::collections::BTreeSet<_> = parent
            .read_dir()
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            sync_removal_targets(&f.targets, &changes(), today()),
            RemovalOutcome::Warning {
                stage: RemovalStage::PdfRender
            }
        );
        assert_eq!(
            std::fs::read(&f.targets.pdf).unwrap(),
            b"old printable",
            "{script}"
        );
        let after: std::collections::BTreeSet<_> = parent
            .read_dir()
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(before, after, "{script}");
    }
}

#[test]
fn removal_retries_existing_printable_when_markdown_unchanged_and_strips_only_render_copy() {
    let mut f = fixture(Some(AGENDA));
    assert_eq!(
        sync_removal_targets(&f.targets, &changes(), today()),
        RemovalOutcome::Updated { pdf: false }
    );
    let text = std::fs::read_to_string(&f.targets.markdown).unwrap()
        + "## Appendix <!-- brain:optional-content -->\n\nKeep notes.\n";
    std::fs::write(&f.targets.markdown, &text).unwrap();
    std::fs::write(&f.targets.pdf, b"stale printable").unwrap();
    f.targets.renderer = Some(fake_renderer(f.targets.pdf.parent().unwrap()));
    assert_eq!(
        sync_removal_targets(&f.targets, &changes(), today()),
        RemovalOutcome::Updated { pdf: true }
    );
    assert_eq!(std::fs::read_to_string(&f.targets.markdown).unwrap(), text);
    let printable = std::fs::read_to_string(&f.targets.pdf).unwrap();
    assert!(printable.contains("Keep notes.") && !printable.contains("<!--"));
    assert!(!f.targets.markdown.with_extension("render.md").exists());
}

#[test]
fn removal_invalid_directory_output_cleans_only_its_staging_and_retains_previous_printable() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = fixture(Some(AGENDA));
    let parent = f.targets.pdf.parent().unwrap().to_owned();
    let renderer = parent.join("renderer");
    std::fs::write(
        &renderer,
        "#!/bin/sh\nmkdir \"$3\"\nprintf scratch > \"$3/nested\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&renderer, std::fs::Permissions::from_mode(0o755)).unwrap();
    f.targets.renderer = Some(renderer);
    std::fs::write(&f.targets.pdf, b"old printable").unwrap();
    let unrelated = parent.join("unrelated.render.md");
    std::fs::write(&unrelated, b"keep").unwrap();
    let before: std::collections::BTreeSet<_> = parent
        .read_dir()
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        sync_removal_targets(&f.targets, &changes(), today()),
        RemovalOutcome::Warning {
            stage: RemovalStage::PdfRender
        }
    );
    assert_eq!(std::fs::read(&f.targets.pdf).unwrap(), b"old printable");
    assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");
    let after: std::collections::BTreeSet<_> = parent
        .read_dir()
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(before, after);
}
