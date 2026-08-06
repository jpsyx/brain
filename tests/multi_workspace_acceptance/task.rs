use std::process::Command;

use brain::server::receiver::InboundJob;
use brain::tasks::task::load_tasks;

use super::setup::Scenario;

pub(crate) struct FakeAgentTaskTransport<'a> {
    scenario: &'a Scenario,
}

impl<'a> FakeAgentTaskTransport<'a> {
    pub(crate) const fn new(scenario: &'a Scenario) -> Self {
        Self { scenario }
    }

    pub(crate) fn create_from(&self, job: &InboundJob) {
        let temporary = self.scenario.home.join("tmp");
        std::fs::create_dir_all(&temporary).expect("temporary command directory");
        let mut command = Command::new("python3");
        command
            .arg(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("skills/todo/scripts/add_task.py"),
            )
            .args([
                "--name",
                "Buy groceries",
                "--type",
                "personal",
                "--priority",
                "p2",
            ])
            .env("HOME", &self.scenario.home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_CACHE_HOME")
            .env("TMPDIR", &temporary)
            .env("PYTHONDONTWRITEBYTECODE", "1");
        for (key, value) in self.scenario.family.integration_env(&job.actor) {
            command.env(key, value);
        }
        let output = command.output().expect("fake agent task boundary");
        assert!(
            output.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let tasks =
            load_tasks(&self.scenario.family.root().join("tasks/tasks.csv")).expect("family tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "Buy groceries");
        assert_eq!(tasks[0].assigned_to, "wife");
    }
}
