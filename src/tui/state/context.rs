use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::server::{IngressId, lifecycle::LeaseId};
use crate::session::AgentKind;
use crate::workspace::{CommandContext, WorkspaceContext};

pub(crate) struct AppContextInit {
    pub(crate) command: CommandContext,
    pub(crate) config: Config,
    pub(crate) agent_kind: AgentKind,
    pub(crate) agent_command: String,
    pub(crate) csv_path: PathBuf,
    pub(crate) log_path: Option<PathBuf>,
    pub(crate) server_ingress: IngressId,
    pub(crate) server_local_capability: LeaseId,
}

pub(crate) struct AppContext {
    command: CommandContext,
    config: Config,
    agent_kind: AgentKind,
    agent_command: String,
    csv_path: PathBuf,
    brain_root: PathBuf,
    db_path: PathBuf,
    log_path: Option<PathBuf>,
    server_ingress: IngressId,
    server_local_capability: LeaseId,
}

impl AppContext {
    pub(crate) fn new(init: AppContextInit) -> Self {
        let brain_root = init.command.workspace.root().to_path_buf();
        let db_path = init.command.workspace.paths().state_db();
        Self {
            command: init.command,
            config: init.config,
            agent_kind: init.agent_kind,
            agent_command: init.agent_command,
            csv_path: init.csv_path,
            brain_root,
            db_path,
            log_path: init.log_path,
            server_ingress: init.server_ingress,
            server_local_capability: init.server_local_capability,
        }
    }

    #[must_use]
    pub(crate) const fn command(&self) -> &CommandContext {
        &self.command
    }

    #[must_use]
    pub(crate) fn workspace(&self) -> &WorkspaceContext {
        &self.command.workspace
    }

    #[must_use]
    pub(crate) const fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub(crate) const fn agent_kind(&self) -> AgentKind {
        self.agent_kind
    }

    #[must_use]
    pub(crate) fn agent_command(&self) -> &str {
        &self.agent_command
    }

    #[must_use]
    pub(crate) fn tasks_csv_path(&self) -> &Path {
        &self.csv_path
    }

    #[must_use]
    pub(crate) fn workspace_root(&self) -> &Path {
        &self.brain_root
    }

    #[must_use]
    pub(crate) fn state_db_path(&self) -> &Path {
        &self.db_path
    }

    #[must_use]
    pub(crate) fn log_path(&self) -> Option<&Path> {
        self.log_path.as_deref()
    }

    #[must_use]
    pub(crate) const fn access_mode(&self) -> crate::access::AccessMode {
        self.config.access_mode
    }

    #[must_use]
    pub(crate) const fn triage_habits_enabled(&self) -> bool {
        self.config.enable_triage_habits
    }

    #[must_use]
    pub(crate) fn daily_triage_pattern(&self) -> &str {
        &self.config.daily_triage_name_pattern
    }

    #[must_use]
    pub(crate) const fn day_rollover_hour(&self) -> u32 {
        self.config.day_rollover_hour
    }

    #[must_use]
    pub(crate) fn linear_base_url(&self) -> String {
        self.config.linear_base_url()
    }

    #[must_use]
    pub(crate) fn replacing_config(&self, config: Config) -> Self {
        Self {
            command: self.command.clone(),
            config,
            agent_kind: self.agent_kind,
            agent_command: self.agent_command.clone(),
            csv_path: self.csv_path.clone(),
            brain_root: self.brain_root.clone(),
            db_path: self.db_path.clone(),
            log_path: self.log_path.clone(),
            server_ingress: self.server_ingress,
            server_local_capability: self.server_local_capability,
        }
    }

    #[must_use]
    pub(crate) fn habits_url(&self, port: u16) -> String {
        crate::server::habits_url(port, self.server_ingress, self.server_local_capability)
    }

    #[must_use]
    pub(crate) fn session_done_url(&self, port: u16) -> String {
        crate::server::url(
            port,
            &crate::server::session_done_path(self.server_ingress, self.server_local_capability),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::access::AccessMode;
    use crate::config::Config;
    use crate::server::{IngressId, lifecycle::LeaseId};
    use crate::session::AgentKind;
    use crate::workspace::{
        CommandContext, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
    };

    use super::{AppContext, AppContextInit};

    fn command_context(home: &Path) -> CommandContext {
        let workspace = WorkspaceContext::new(
            home,
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("workspace id"),
            WorkspaceName::parse("family").expect("workspace name"),
            Path::new("/workspaces/family"),
            "tester",
            Path::new("/workspaces"),
        )
        .expect("workspace context");
        CommandContext::for_test(
            Arc::new(workspace),
            RegistryStore::from_path(home.join(".config/brain/env.json")),
            "tester",
        )
    }

    fn context(home: &Path, config: Config) -> AppContext {
        AppContext::new(AppContextInit {
            command: command_context(home),
            config,
            agent_kind: AgentKind::Codex,
            agent_command: "codex --profile brain".to_owned(),
            csv_path: PathBuf::from("/workspaces/family/tasks/tasks.csv"),
            log_path: Some(PathBuf::from("/tmp/brain-test.log")),
            server_ingress: IngressId::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
                .expect("ingress"),
            server_local_capability: LeaseId::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
                .expect("local capability"),
        })
    }

    #[test]
    fn context_derives_workspace_paths_and_pins_frontend_identity() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let context = context(temporary.path(), Config::default());

        assert_eq!(context.workspace_root(), Path::new("/workspaces/family"));
        assert_eq!(
            context.state_db_path(),
            temporary
                .path()
                .join(".cache/brain/workspaces/8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b/state.db")
        );
        assert_eq!(
            context.tasks_csv_path(),
            Path::new("/workspaces/family/tasks/tasks.csv")
        );
        assert_eq!(context.agent_kind(), AgentKind::Codex);
        assert_eq!(context.agent_command(), "codex --profile brain");
    }

    #[test]
    fn refreshed_config_replaces_one_immutable_snapshot_without_changing_identity() {
        let temporary = tempfile::tempdir().expect("temporary home");
        let original = context(temporary.path(), Config::default());
        let refreshed = original.replacing_config(Config {
            access_mode: AccessMode::WorkspaceOnly,
            day_rollover_hour: 9,
            ..Config::default()
        });

        assert_eq!(original.access_mode(), AccessMode::Unrestricted);
        assert_eq!(refreshed.access_mode(), AccessMode::WorkspaceOnly);
        assert_eq!(refreshed.day_rollover_hour(), 9);
        assert_eq!(original.workspace().id(), refreshed.workspace().id());
        assert_eq!(original.state_db_path(), refreshed.state_db_path());
    }
}
