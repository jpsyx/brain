use std::path::{Path, PathBuf};

/// Filesystem base for one declarative lifecycle installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleTarget {
    /// Path relative to the selected workspace root.
    Workspace(&'static str),
}

/// Command path convention used by a frontend's hook settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookCommandStyle {
    /// Commands resolve through Claude's own project-root variable.
    ClaudeProjectDir,
    /// Commands resolve through the selected workspace's `BRAIN_ROOT`.
    PortableBrainRoot,
}

/// Generic write operation described by the frontend registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecyclePayload {
    /// Write exact bundled source with the requested Unix mode.
    StaticFile { contents: &'static str, mode: u32 },
    /// Merge Brain's normalized session-start and session-stop hooks into JSON settings.
    HookSettings {
        style: HookCommandStyle,
        session_script: &'static str,
        completion_script: &'static str,
        legacy_session_scripts: &'static [&'static str],
        legacy_completion_scripts: &'static [&'static str],
    },
}

/// One declarative Brain-owned lifecycle artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LifecycleInstallation {
    id: &'static str,
    target: LifecycleTarget,
    payload: LifecyclePayload,
}

impl LifecycleInstallation {
    #[must_use]
    pub(crate) const fn id(self) -> &'static str {
        self.id
    }

    #[must_use]
    pub(crate) const fn payload(self) -> LifecyclePayload {
        self.payload
    }

    #[must_use]
    pub(crate) fn path(self, root: &Path, _home: &Path) -> PathBuf {
        match self.target {
            LifecycleTarget::Workspace(relative) => root.join(relative),
        }
    }

    #[must_use]
    pub(crate) fn auxiliary_paths(self, root: &Path, home: &Path) -> Vec<PathBuf> {
        let path = self.path(root, home);
        match self.payload {
            LifecyclePayload::HookSettings { .. } => {
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("hooks.json");
                vec![path.with_file_name(format!(".{file_name}.transaction.lock"))]
            }
            LifecyclePayload::StaticFile { .. } => Vec::new(),
        }
    }
}

/// Location inspected by a frontend lifecycle health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HealthCheckTarget {
    /// A selected-workspace-relative file.
    WorkspaceFile(&'static str),
}

/// Evidence expected at one health-check location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HealthCheckExpectation {
    /// One configured lifecycle event invokes a command ending with this script name.
    Hook {
        event: &'static str,
        suffix: &'static str,
    },
    /// A regular file has the exact Brain-owned source currently bundled.
    FileContents(&'static str),
}

/// One read-only health check for a frontend integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HealthCheckDescriptor {
    label: &'static str,
    target: HealthCheckTarget,
    expectation: HealthCheckExpectation,
}

impl HealthCheckDescriptor {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        self.label
    }

    #[must_use]
    pub(crate) fn path(self, root: &Path, _home: &Path) -> PathBuf {
        match self.target {
            HealthCheckTarget::WorkspaceFile(relative) => root.join(relative),
        }
    }

    #[must_use]
    pub(crate) const fn expectation(self) -> HealthCheckExpectation {
        self.expectation
    }
}

const SESSION_START_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/agent_session_start_hook.py"
));
const SESSION_STOP_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/agent_session_stop_hook.py"
));
const OPENCODE_PLUGIN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/opencode_brain_plugin.js"
));
pub(super) const CLAUDE_LIFECYCLE: [LifecycleInstallation; 3] = [
    LifecycleInstallation {
        id: "agent-session-start-script",
        target: LifecycleTarget::Workspace(".brain/hooks/agent_session_start_hook.py"),
        payload: LifecyclePayload::StaticFile {
            contents: SESSION_START_SCRIPT,
            mode: 0o755,
        },
    },
    LifecycleInstallation {
        id: "agent-session-stop-script",
        target: LifecycleTarget::Workspace(".brain/hooks/agent_session_stop_hook.py"),
        payload: LifecyclePayload::StaticFile {
            contents: SESSION_STOP_SCRIPT,
            mode: 0o755,
        },
    },
    LifecycleInstallation {
        id: "claude-settings",
        target: LifecycleTarget::Workspace(".claude/settings.json"),
        payload: LifecyclePayload::HookSettings {
            style: HookCommandStyle::ClaudeProjectDir,
            session_script: ".brain/hooks/agent_session_start_hook.py",
            completion_script: ".brain/hooks/agent_session_stop_hook.py",
            legacy_session_scripts: &["claude_session_start_hook.py"],
            legacy_completion_scripts: &["claude_stop_hook.py", "agent_turn_complete_hook.py"],
        },
    },
];

pub(super) const CODEX_LIFECYCLE: [LifecycleInstallation; 1] = [LifecycleInstallation {
    id: "codex-settings",
    target: LifecycleTarget::Workspace(".codex/hooks.json"),
    payload: LifecyclePayload::HookSettings {
        style: HookCommandStyle::PortableBrainRoot,
        session_script: ".brain/hooks/agent_session_start_hook.py",
        completion_script: ".brain/hooks/agent_session_stop_hook.py",
        legacy_session_scripts: &["claude_session_start_hook.py"],
        legacy_completion_scripts: &["claude_stop_hook.py", "agent_turn_complete_hook.py"],
    },
}];

pub(super) const OPENCODE_LIFECYCLE: [LifecycleInstallation; 1] = [LifecycleInstallation {
    id: "opencode-plugin",
    target: LifecycleTarget::Workspace(".opencode/plugins/brain.js"),
    payload: LifecyclePayload::StaticFile {
        contents: OPENCODE_PLUGIN,
        mode: 0o644,
    },
}];

pub(super) const CLAUDE_HEALTH: [HealthCheckDescriptor; 4] = [
    HealthCheckDescriptor {
        label: "SessionStart",
        target: HealthCheckTarget::WorkspaceFile(".claude/settings.json"),
        expectation: HealthCheckExpectation::Hook {
            event: "SessionStart",
            suffix: ".brain/hooks/agent_session_start_hook.py",
        },
    },
    HealthCheckDescriptor {
        label: "Stop",
        target: HealthCheckTarget::WorkspaceFile(".claude/settings.json"),
        expectation: HealthCheckExpectation::Hook {
            event: "Stop",
            suffix: ".brain/hooks/agent_session_stop_hook.py",
        },
    },
    HealthCheckDescriptor {
        label: "session-start bridge",
        target: HealthCheckTarget::WorkspaceFile(".brain/hooks/agent_session_start_hook.py"),
        expectation: HealthCheckExpectation::FileContents(SESSION_START_SCRIPT),
    },
    HealthCheckDescriptor {
        label: "session-stop bridge",
        target: HealthCheckTarget::WorkspaceFile(".brain/hooks/agent_session_stop_hook.py"),
        expectation: HealthCheckExpectation::FileContents(SESSION_STOP_SCRIPT),
    },
];

pub(super) const CODEX_HEALTH: [HealthCheckDescriptor; 4] = [
    HealthCheckDescriptor {
        label: "SessionStart",
        target: HealthCheckTarget::WorkspaceFile(".codex/hooks.json"),
        expectation: HealthCheckExpectation::Hook {
            event: "SessionStart",
            suffix: ".brain/hooks/agent_session_start_hook.py",
        },
    },
    HealthCheckDescriptor {
        label: "Stop",
        target: HealthCheckTarget::WorkspaceFile(".codex/hooks.json"),
        expectation: HealthCheckExpectation::Hook {
            event: "Stop",
            suffix: ".brain/hooks/agent_session_stop_hook.py",
        },
    },
    HealthCheckDescriptor {
        label: "session-start bridge",
        target: HealthCheckTarget::WorkspaceFile(".brain/hooks/agent_session_start_hook.py"),
        expectation: HealthCheckExpectation::FileContents(SESSION_START_SCRIPT),
    },
    HealthCheckDescriptor {
        label: "session-stop bridge",
        target: HealthCheckTarget::WorkspaceFile(".brain/hooks/agent_session_stop_hook.py"),
        expectation: HealthCheckExpectation::FileContents(SESSION_STOP_SCRIPT),
    },
];

pub(super) const OPENCODE_HEALTH: [HealthCheckDescriptor; 3] = [
    HealthCheckDescriptor {
        label: "Brain plugin",
        target: HealthCheckTarget::WorkspaceFile(".opencode/plugins/brain.js"),
        expectation: HealthCheckExpectation::FileContents(OPENCODE_PLUGIN),
    },
    HealthCheckDescriptor {
        label: "session-start bridge",
        target: HealthCheckTarget::WorkspaceFile(".brain/hooks/agent_session_start_hook.py"),
        expectation: HealthCheckExpectation::FileContents(SESSION_START_SCRIPT),
    },
    HealthCheckDescriptor {
        label: "session-stop bridge",
        target: HealthCheckTarget::WorkspaceFile(".brain/hooks/agent_session_stop_hook.py"),
        expectation: HealthCheckExpectation::FileContents(SESSION_STOP_SCRIPT),
    },
];
