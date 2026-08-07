use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Map, json};

use super::*;
use crate::users::{User, UserId, Users, UsersStore};
use crate::workspace::{
    CommandContext, MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
    WorkspaceRecord,
};

struct Fixture {
    directory: tempfile::TempDir,
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
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::write(
            root.join(".config/.receiver-setup.transaction.lock"),
            b"stable setup lock\n",
        )
        .unwrap();
        let context = CommandContext::new(workspace, store).unwrap();
        Self {
            registry_before: std::fs::read(context.registry_store.path()).unwrap(),
            users_before: std::fs::read(UsersStore::path(&context.workspace)).unwrap(),
            codex_before: std::fs::read(codex).unwrap(),
            directory,
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
            ".claude/brain-hooks/agent_session_start_hook.py",
            ".claude/brain-hooks/agent_turn_complete_hook.py",
            ".claude/brain-hooks/claude_session_start_hook.py",
            ".claude/brain-hooks/claude_stop_hook.py",
            ".claude/settings.json",
            ".opencode/plugins/brain.js",
        ] {
            assert!(!self.context.workspace.root().join(path).exists(), "{path}");
        }
    }

    fn tree_snapshot(&self) -> BTreeMap<std::path::PathBuf, TreeEntry> {
        fn visit(
            base: &std::path::Path,
            path: &std::path::Path,
            entries: &mut BTreeMap<std::path::PathBuf, TreeEntry>,
        ) {
            use std::os::unix::fs::PermissionsExt as _;

            let mut children = std::fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                let relative = child.strip_prefix(base).unwrap().to_path_buf();
                let metadata = std::fs::symlink_metadata(&child).unwrap();
                let mode = metadata.permissions().mode() & 0o777;
                let entry = if metadata.file_type().is_symlink() {
                    TreeEntry::Symlink(std::fs::read_link(&child).unwrap())
                } else if metadata.is_dir() {
                    TreeEntry::Directory(mode)
                } else {
                    TreeEntry::File {
                        bytes: std::fs::read(&child).unwrap(),
                        mode,
                    }
                };
                entries.insert(relative, entry);
                if metadata.is_dir() {
                    visit(base, &child, entries);
                }
            }
        }

        let mut entries = BTreeMap::new();
        visit(self.directory.path(), self.directory.path(), &mut entries);
        entries
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TreeEntry {
    Directory(u32),
    File { bytes: Vec<u8>, mode: u32 },
    Symlink(std::path::PathBuf),
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
        CommitStep::Directory("agent-session-start-script"),
        CommitStep::Hook("agent-session-start-script"),
        CommitStep::Directory("agent-turn-complete-script"),
        CommitStep::Hook("agent-turn-complete-script"),
        CommitStep::Directory("claude-session-start-compatibility-script"),
        CommitStep::Hook("claude-session-start-compatibility-script"),
        CommitStep::Directory("claude-stop-compatibility-script"),
        CommitStep::Hook("claude-stop-compatibility-script"),
        CommitStep::Directory("claude-settings"),
        CommitStep::Lock("claude-settings"),
        CommitStep::Hook("claude-settings"),
        CommitStep::Directory("codex-settings"),
        CommitStep::Lock("codex-settings"),
        CommitStep::Hook("codex-settings"),
        CommitStep::Directory("opencode-plugin"),
        CommitStep::Hook("opencode-plugin"),
    ] {
        let fixture = Fixture::new();
        let before = fixture.tree_snapshot();
        let error = persist_plan_with_hook(&plan(), &fixture.context, &fixture.home, |step| {
            anyhow::ensure!(step != target, "injected {target:?} failure");
            Ok(())
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected"), "{error:#}");
        fixture.assert_restored();
        assert_eq!(fixture.tree_snapshot(), before, "failed at {target:?}");
    }
}

#[test]
fn setup_rejects_an_external_bridge_symlink_and_preserves_its_target_exactly() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::new();
    let hook_dir = fixture.context.workspace.root().join(".claude/brain-hooks");
    std::fs::create_dir_all(&hook_dir).unwrap();
    let target = fixture.home.join("rendered-session-bridge.py");
    let middle = fixture.home.join("current-session-bridge.py");
    let original = b"original rendered bridge\n";
    std::fs::write(&target, original).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    std::os::unix::fs::symlink(&target, &middle).unwrap();
    let link = hook_dir.join("agent_session_start_hook.py");
    std::os::unix::fs::symlink(&middle, &link).unwrap();

    let error =
        persist_plan_with_hook(&plan(), &fixture.context, &fixture.home, |_| Ok(())).unwrap_err();

    assert!(
        error.to_string().contains("resolves outside workspace"),
        "{error:#}"
    );
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), middle);
    assert!(
        std::fs::symlink_metadata(&middle)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_link(&middle).unwrap(), target);
    assert_eq!(std::fs::read(&link).unwrap(), original);
    assert_eq!(
        std::fs::metadata(&link).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn setup_rejects_an_external_dangling_bridge_symlink_without_creating_its_target() {
    let fixture = Fixture::new();
    let hook_dir = fixture.context.workspace.root().join(".claude/brain-hooks");
    std::fs::create_dir_all(&hook_dir).unwrap();
    let target = fixture.home.join("not-yet-rendered-session-bridge.py");
    let middle = fixture.home.join("current-session-bridge.py");
    std::os::unix::fs::symlink(&target, &middle).unwrap();
    let link = hook_dir.join("agent_session_start_hook.py");
    std::os::unix::fs::symlink(&middle, &link).unwrap();

    let error =
        persist_plan_with_hook(&plan(), &fixture.context, &fixture.home, |_| Ok(())).unwrap_err();

    assert!(
        error.to_string().contains("resolves outside workspace"),
        "{error:#}"
    );
    assert!(!target.exists());
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), middle);
    assert!(
        std::fs::symlink_metadata(&middle)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_link(&middle).unwrap(), target);
}

#[test]
fn relative_path_symlink_cycle_is_rejected_at_a_bounded_depth() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir(temporary.path().join("sub")).unwrap();
    let cycle = temporary.path().join("a");
    std::os::unix::fs::symlink("sub/../a", &cycle).unwrap();

    let error = resolve_symlink_chain(&cycle).unwrap_err();

    assert!(error.to_string().contains("safe depth"), "{error:#}");
}

mod locking;
