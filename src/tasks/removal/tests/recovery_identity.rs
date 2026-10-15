use super::{
    external,
    fixture::{WorkspaceFixture, needles},
};
use crate::tasks::{
    removal::{execute::apply_with_hook, model::Options, operations, preview, resume},
    store_transaction::TransactionStep,
};

#[test]
fn persisted_apply_errors_retain_recovery_id_after_external_or_local_effects() {
    for external_effect in [false, true] {
        let fixture = if external_effect {
            external::fixture("")
        } else {
            WorkspaceFixture::new()
        };
        let context = fixture.command();
        let date = chrono::Local::now().date_naive();
        let plan = preview(&context, &needles(&["T17"]), Options::default(), date).unwrap();
        let directory = context.workspace.paths().task_removals_dir();
        let mut damaged = None;
        let error = apply_with_hook(&context, &plan, &plan.fingerprint, date, |step| {
            let trigger = if external_effect {
                TransactionStep::Stage(0)
            } else {
                TransactionStep::RemoveJournal
            };
            if step == trigger && damaged.is_none() {
                let id = std::fs::read_dir(&directory)
                    .unwrap()
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| {
                        p.extension().is_some_and(|e| e == "json")
                            && !p.to_string_lossy().ends_with(".commit.json")
                    })
                    .unwrap()
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                let receipt = directory.join(format!("{id}.commit.json"));
                let backup = receipt.with_extension("held");
                if receipt.exists() {
                    std::fs::rename(&receipt, &backup).unwrap();
                }
                std::fs::create_dir(&receipt).unwrap();
                damaged = Some((id, receipt, backup));
                return Err(std::io::Error::other("injected receipt read failure"));
            }
            Ok(())
        })
        .unwrap_err();
        let (id, receipt, backup) = damaged.unwrap();
        assert_eq!(error.code, "operation_unavailable");
        assert_eq!(error.details["operation_id"], id);
        assert_eq!(
            resume(&context, &id).unwrap_err().details["operation_id"],
            id
        );
        let saved = operations::load(&context.workspace, &id).unwrap();
        assert_eq!(saved.operation_id, id);
        if external_effect {
            assert!(external::calls(&fixture).contains("cancel EX-1"));
        }
        std::fs::remove_dir(&receipt).unwrap();
        if backup.exists() {
            std::fs::rename(backup, receipt).unwrap();
        }
        let result = resume(&context, &id).unwrap();
        assert!(result.committed);
        assert_eq!(result.operation_id, id);
        if external_effect {
            assert_eq!(external::calls(&fixture).matches("cancel EX-1").count(), 1);
        }
    }
}
