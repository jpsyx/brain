use std::sync::{Arc, Mutex};

use super::{ReceiverIntentRefresher, run_configuration_command};

struct RecordingRefresh(Arc<Mutex<Vec<crate::workspace::WorkspaceId>>>);

impl ReceiverIntentRefresher for RecordingRefresh {
    fn refresh_enabled(&self, workspace_id: crate::workspace::WorkspaceId) -> anyhow::Result<()> {
        self.0.lock().unwrap().push(workspace_id);
        Ok(())
    }
}

#[test]
fn every_saved_configuration_notifies_only_the_selected_workspace() {
    let selected =
        crate::workspace::WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
    let peer =
        crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let refresher = RecordingRefresh(Arc::clone(&calls));
    let mut setup_saved = false;
    let mut set_saved = false;

    run_configuration_command(selected, &refresher, || {
        setup_saved = true;
        Ok(())
    })
    .unwrap();
    run_configuration_command(selected, &refresher, || {
        set_saved = true;
        Ok(())
    })
    .unwrap();

    assert!(setup_saved && set_saved);
    assert_eq!(*calls.lock().unwrap(), [selected, selected]);
    assert!(!calls.lock().unwrap().contains(&peer));
}

#[test]
fn failed_configuration_transaction_never_notifies_the_live_server() {
    let selected =
        crate::workspace::WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let refresher = RecordingRefresh(Arc::clone(&calls));

    let error = run_configuration_command(selected, &refresher, || {
        anyhow::bail!("injected setup transaction failure")
    })
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("injected setup transaction failure")
    );
    assert!(calls.lock().unwrap().is_empty());
}
