use crate::tasks::complete::{CsvFile, Row};

pub(super) fn row(id: &str, name: &str) -> Row {
    Row::from([
        ("task_id".into(), id.into()),
        ("task_name".into(), name.into()),
    ])
}

pub(super) fn csv(rows: Vec<Row>) -> CsvFile {
    CsvFile {
        header: vec!["task_id".into(), "task_name".into()],
        rows,
    }
}

pub(super) fn needles(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

pub(super) struct WorkspaceFixture {
    pub temporary: tempfile::TempDir,
    pub workspace: crate::workspace::WorkspaceContext,
    pub actor: crate::actor::ActorContext,
    pub integration: crate::tasks::removal::model::IntegrationSnapshot,
}

impl WorkspaceFixture {
    pub fn new() -> Self {
        use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let workspace = WorkspaceContext::new(
            temporary.path(),
            WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
            WorkspaceName::parse("example").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();
        let fixture = Self {
            temporary,
            workspace,
            actor: crate::actor::test_actor("member"),
            integration: crate::tasks::removal::model::IntegrationSnapshot::default(),
        };
        fixture.write(
            "tasks/tasks.csv",
            b"task_id,task_name,project\nT17,Send proposal,\n",
        );
        fixture.write("tasks/habits.csv", b"task_id,task_name\nH2,Walk\n");
        fixture
    }

    pub fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.workspace.root().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    pub fn context(&self) -> crate::tasks::removal::snapshot::SnapshotContext<'_> {
        crate::tasks::removal::snapshot::SnapshotContext {
            workspace: &self.workspace,
            actor: &self.actor,
            integration: &self.integration,
        }
    }

    pub fn project(&self, directory: &str) {
        let slug = directory.rsplit('/').next().unwrap();
        let mut metadata =
            crate::project::model::Metadata::new(slug, "Example", "in-progress", "p2", "none");
        metadata.directory = directory.into();
        metadata.tasks = vec!["T17".into()];
        self.write(
            &format!("{directory}/.METADATA.json"),
            &serde_json::to_vec(&metadata).unwrap(),
        );
    }
}

pub(super) fn today() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
}

impl WorkspaceFixture {
    pub fn command(&self) -> crate::workspace::CommandContext {
        crate::workspace::CommandContext::for_test(
            std::sync::Arc::new(self.workspace.clone()),
            crate::workspace::RegistryStore::from_path(self.temporary.path().join("registry.json")),
            "member",
        )
    }
}
