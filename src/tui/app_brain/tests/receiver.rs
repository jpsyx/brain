use super::*;

#[derive(Clone)]
struct RecordingReceiverRefresh {
    calls: Arc<Mutex<Vec<WorkspaceId>>>,
    fail: bool,
}

impl crate::command::server::ReceiverIntentRefresher for RecordingReceiverRefresh {
    fn refresh_enabled(&self, workspace_id: WorkspaceId) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(workspace_id);
        if self.fail {
            anyhow::bail!("control refresh failed")
        }
        Ok(())
    }
}

fn seed_receiver_registry(app: &App) -> WorkspaceName {
    let selected_name = app.context.workspace().name().clone();
    let peer_name = WorkspaceName::parse("personal").unwrap();
    let selected = crate::workspace::WorkspaceRecord {
        workspace_id: app.context.workspace().id(),
        root: app.context.workspace().root().to_path_buf(),
        aliases: std::collections::BTreeSet::new(),
        local_user_id: app.context.workspace().local_user_id().to_owned(),
        receiver_enabled: false,
        env: serde_json::Map::new(),
    };
    let peer = crate::workspace::WorkspaceRecord {
        workspace_id: WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
        root: app.context.workspace().root().with_file_name("personal"),
        aliases: std::collections::BTreeSet::new(),
        local_user_id: "peer".to_owned(),
        receiver_enabled: false,
        env: serde_json::Map::new(),
    };
    app.context
        .command()
        .registry_store
        .replace(&crate::workspace::MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: selected_name.clone(),
            workspaces: std::collections::BTreeMap::from([
                (selected_name, selected),
                (peer_name.clone(), peer),
            ]),
            env: serde_json::Map::new(),
        })
        .unwrap();
    peer_name
}

fn plain_key(code: KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

#[test]
fn tasks_and_search_palettes_persist_both_directions_and_refresh_exact_workspace() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let peer_name = seed_receiver_registry(&app);
    let calls = Arc::new(Mutex::new(Vec::new()));
    app.services
        .replace_receiver_intent_refresher(Box::new(RecordingReceiverRefresh {
            calls: Arc::clone(&calls),
            fail: false,
        }));
    app.overlay = Some(crate::tui::overlay::Overlay::TaskPalette(
        crate::tui::modal_state::TaskPalette::new(
            None,
            false,
            false,
            false,
            crate::tui::links::LinkKind::None,
            false,
            false,
        ),
    ));
    for character in "enable receiver".chars() {
        crate::tui::handlers::handle_palette_key(
            &mut app,
            &plain_key(KeyCode::Char(character)),
            false,
        );
    }
    crate::tui::handlers::handle_palette_key(&mut app, &plain_key(KeyCode::Enter), false);

    assert!(app.receiver.is_enabled());
    let saved = RegistryStore::load_from(app.context.command().registry_store.path()).unwrap();
    assert!(saved.workspaces[app.context.workspace().name()].receiver_enabled);
    assert!(!saved.workspaces[&peer_name].receiver_enabled);
    assert_eq!(*calls.lock().unwrap(), [app.context.workspace().id()]);

    app.services
        .replace_receiver_intent_refresher(Box::new(RecordingReceiverRefresh {
            calls: Arc::clone(&calls),
            fail: true,
        }));
    app.overlay = Some(crate::tui::overlay::Overlay::SearchPalette(
        app.shell.search_palette(false, app.receiver.is_enabled()),
    ));
    for character in "disable receiver".chars() {
        crate::tui::search_view::route_search_palette(
            &mut app,
            &plain_key(KeyCode::Char(character)),
        );
    }
    crate::tui::search_view::route_search_palette(&mut app, &plain_key(KeyCode::Enter));

    assert!(!app.receiver.is_enabled());
    assert!(matches!(
        app.status.flash(),
        Some(crate::tui::modal_state::FlashKind::Error(message))
            if message.contains("receiver disabled; warning:")
    ));
    let saved = RegistryStore::load_from(app.context.command().registry_store.path()).unwrap();
    assert!(!saved.workspaces[app.context.workspace().name()].receiver_enabled);
    assert!(!saved.workspaces[&peer_name].receiver_enabled);
    assert_eq!(
        *calls.lock().unwrap(),
        [app.context.workspace().id(), app.context.workspace().id(),]
    );
}
