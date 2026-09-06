use super::super::*;

pub(crate) fn live_panel(root: &Path) -> PtyPane {
    PtyPane::spawn_shell_command_with_env("cat", &[], root, 24, 80).expect("spawn panel")
}

pub(crate) fn panel_controller(app: &App, panel: PtyPane) -> AgentController {
    panel_controller_for_actor(app, app.brain.interactive_actor().clone(), panel)
}

pub(crate) fn panel_controller_for_actor(
    app: &App,
    actor: crate::actor::ActorContext,
    panel: PtyPane,
) -> AgentController {
    AgentController::configured(
        app.context.command(),
        app.context.agent_kind(),
        actor,
        Box::new(panel),
    )
}

pub(crate) fn capture_panel(root: &Path) -> PtyPane {
    PtyPane::spawn_shell_command_with_env(
        "stty raw -echo; printf READY; dd bs=1 count=5 2>/dev/null | od -An -t x1",
        &[],
        root,
        24,
        80,
    )
    .expect("spawn capture panel")
}

pub(crate) fn wait_for_panel_contents(panel: &AgentController, expected: &str) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let normalized = panel
            .snapshot()
            .expect("supported test panel snapshot")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if normalized.contains(expected) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub(crate) struct ClaudeTranscript {
    path: PathBuf,
    project_dir: PathBuf,
}

impl ClaudeTranscript {
    pub(crate) fn create(brain_root: &Path, session_id: &str) -> Self {
        let home = std::env::var_os("HOME").expect("test home directory");
        let project_dir = PathBuf::from(home)
            .join(".claude/projects")
            .join(session::project_dir_name(brain_root));
        std::fs::create_dir_all(&project_dir).expect("create transcript directory");
        let path = project_dir.join(format!("{session_id}.jsonl"));
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
                "\n",
                r#"{"type":"assistant","message":{"role":"assistant","content":"hello"}}"#,
                "\n",
            ),
        )
        .expect("write Claude transcript");
        Self { path, project_dir }
    }
}

impl Drop for ClaudeTranscript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.project_dir);
    }
}
