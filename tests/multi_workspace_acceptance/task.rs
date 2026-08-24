use std::process::Command;

use brain::server::receiver::InboundJob;
use brain::tasks::task::load_tasks;

use super::setup::Scenario;

/// A stand-in for the agent that answers an inbound message by creating a task.
///
/// It runs the **real binary** with the workspace's integration environment,
/// which is the contract that matters here: `BRAIN_WORKSPACE` and
/// `BRAIN_ACTOR_ID` have to route the create to the addressed workspace and
/// attribute it to the sender, without the caller naming either one.
pub(crate) struct FakeAgentTaskTransport<'a> {
    scenario: &'a Scenario,
}

impl<'a> FakeAgentTaskTransport<'a> {
    pub(crate) const fn new(scenario: &'a Scenario) -> Self {
        Self { scenario }
    }

    fn brain(&self, job: &InboundJob) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
        command
            .env("HOME", &self.scenario.home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_CACHE_HOME")
            .env("NO_COLOR", "1");
        for (key, value) in self.scenario.family.integration_env(&job.actor) {
            command.env(key, value);
        }
        command
    }

    pub(crate) fn create_from(&self, job: &InboundJob) {
        let temporary = self.scenario.home.join("tmp");
        std::fs::create_dir_all(&temporary).expect("temporary command directory");
        // `agenda_markdown_dir` defaults to the machine-shared `/tmp`, which a
        // temporary HOME does not redirect — see docs/testing.md.
        let isolate = self
            .brain(job)
            .args([
                "env",
                "set",
                &format!("agenda_markdown_dir={}", temporary.display()),
            ])
            .output()
            .expect("isolate the agenda directory");
        assert!(
            isolate.status.success(),
            "{}",
            String::from_utf8_lossy(&isolate.stderr)
        );

        let output = self
            .brain(job)
            .args([
                "tasks",
                "add",
                "--name",
                "Buy groceries",
                "--type",
                "personal",
                "--priority",
                "p2",
            ])
            .output()
            .expect("fake agent task boundary");
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
