use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::Barrier;

use serde_json::{Map, json};

use super::*;
use crate::command::server::receiver::hooks::InstallStep;
use crate::users::{User, UserId, Users, UsersStore};
use crate::workspace::{
    CommandContext, MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
    WorkspaceRecord,
};

struct Fixture {
    _directory: tempfile::TempDir,
    home: std::path::PathBuf,
    context: CommandContext,
    registry_before: Vec<u8>,
    users_before: Vec<u8>,
    codex_before: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let root = directory.path().join("selected");
        let peer_root = directory.path().join("peer");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&peer_root).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
        let peer_id = WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
        let name = WorkspaceName::parse("selected").unwrap();
        let peer = WorkspaceName::parse("peer").unwrap();
        let registry = MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: name.clone(),
            workspaces: BTreeMap::from([
                (
                    name.clone(),
                    WorkspaceRecord {
                        workspace_id: id,
                        root: root.clone(),
                        aliases: BTreeSet::new(),
                        local_user_id: "pablo".to_owned(),
                        receiver_enabled: false,
                        env: Map::from_iter([("peer_safe".to_owned(), json!("old selected"))]),
                    },
                ),
                (
                    peer,
                    WorkspaceRecord {
                        workspace_id: peer_id,
                        root: peer_root,
                        aliases: BTreeSet::new(),
                        local_user_id: "peer".to_owned(),
                        receiver_enabled: true,
                        env: Map::from_iter([("peer_secret".to_owned(), json!("untouched"))]),
                    },
                ),
            ]),
        };
        let store = RegistryStore::from_path(directory.path().join("config/brain/env.json"));
        store.replace(&registry).unwrap();
        let workspace = Arc::new(
            WorkspaceContext::new(&home, id, name, &root, "pablo", directory.path()).unwrap(),
        );
        UsersStore::save(
            &workspace,
            &Users {
                schema_version: crate::users::USERS_SCHEMA_VERSION,
                users: vec![User {
                    id: UserId::parse("pablo").unwrap(),
                    name: "Pablo".to_owned(),
                    phones: Vec::new(),
                    emails: Vec::new(),
                    response_email: None,
                }],
            },
        )
        .unwrap();
        let codex = home.join(".codex/hooks.json");
        std::fs::write(&codex, b"{\"peer\":true}\n").unwrap();
        let context = CommandContext::new(workspace, store).unwrap();
        Self {
            registry_before: std::fs::read(context.registry_store.path()).unwrap(),
            users_before: std::fs::read(UsersStore::path(&context.workspace)).unwrap(),
            codex_before: std::fs::read(codex).unwrap(),
            _directory: directory,
            home,
            context,
        }
    }

    fn assert_restored(&self) {
        assert_eq!(
            std::fs::read(self.context.registry_store.path()).unwrap(),
            self.registry_before
        );
        assert_eq!(
            std::fs::read(UsersStore::path(&self.context.workspace)).unwrap(),
            self.users_before
        );
        assert_eq!(
            std::fs::read(self.home.join(".codex/hooks.json")).unwrap(),
            self.codex_before
        );
        for path in [
            ".claude/brain-hooks/claude_session_start_hook.py",
            ".claude/brain-hooks/claude_stop_hook.py",
            ".claude/settings.json",
        ] {
            assert!(!self.context.workspace.root().join(path).exists(), "{path}");
        }
    }
}

#[test]
fn rollback_does_not_clobber_an_unexpected_concurrent_file_change() {
    let fixture = Fixture::new();
    let users_path = UsersStore::path(&fixture.context.workspace);

    let error = persist_plan_with_hook(&plan(), &fixture.context, &fixture.home, |step| {
        if step == CommitStep::Providers {
            std::fs::remove_file(&users_path)?;
            std::fs::create_dir(&users_path)?;
            anyhow::bail!("injected provider-boundary failure");
        }
        Ok(())
    })
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(
        message.contains("injected provider-boundary failure"),
        "{message}"
    );
    assert!(!message.contains("rollback also failed"), "{message}");
    assert!(users_path.is_dir());
}

fn plan() -> super::super::SetupPlan {
    super::super::SetupPlan {
        channels: crate::cli::ReceiverSetupChannels::Email,
        providers: vec![
            (
                "brain_receiver_public_url",
                "https://brain.example.test".to_owned(),
            ),
            ("resend_api_key", "re_secret".to_owned()),
            ("resend_from_email", "brain@example.test".to_owned()),
            ("resend_webhook_signing_secret", "whsec_secret".to_owned()),
        ],
        users: Users::empty(),
    }
}

#[test]
fn every_persistence_and_hook_failure_restores_exact_selected_bytes_and_peer_state() {
    for target in [
        CommitStep::Providers,
        CommitStep::Users,
        CommitStep::Hook(InstallStep::SessionScript),
        CommitStep::Hook(InstallStep::StopScript),
        CommitStep::Hook(InstallStep::ClaudeSettings),
        CommitStep::Hook(InstallStep::CodexSettings),
    ] {
        let fixture = Fixture::new();
        let error = persist_plan_with_hook(&plan(), &fixture.context, &fixture.home, |step| {
            anyhow::ensure!(step != target, "injected {target:?} failure");
            Ok(())
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected"), "{error:#}");
        fixture.assert_restored();
    }
}

#[test]
fn failed_setup_rollback_preserves_a_concurrent_success_and_live_lock_inode() {
    use std::os::unix::fs::MetadataExt as _;

    let fixture = Fixture::new();
    let context = fixture.context.clone();
    let home = fixture.home.clone();
    let provider_written = Arc::new(Barrier::new(2));
    let release_failure = Arc::new(Barrier::new(2));
    let thread_provider_written = Arc::clone(&provider_written);
    let thread_release_failure = Arc::clone(&release_failure);
    let lock = home.join(".codex/.hooks.json.transaction.lock");
    std::fs::write(&lock, b"live lock").unwrap();
    let lock_inode = std::fs::metadata(&lock).unwrap().ino();

    let failing = std::thread::spawn(move || {
        persist_plan_with_hook(&plan(), &context, &home, |step| {
            if step == CommitStep::Providers {
                thread_provider_written.wait();
                thread_release_failure.wait();
                anyhow::bail!("injected failure after concurrent success");
            }
            Ok(())
        })
    });
    provider_written.wait();
    crate::env::set(&fixture.context, "codex_cmd", "concurrent-codex").unwrap();
    release_failure.wait();

    assert!(failing.join().unwrap().is_err());
    assert_eq!(
        crate::env::get(&fixture.context, "codex_cmd").as_deref(),
        Some("concurrent-codex")
    );
    assert_eq!(std::fs::metadata(lock).unwrap().ino(), lock_inode);
}
