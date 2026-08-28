//! Workspace-siloed machine-local provider configuration.

use std::io::{Read as _, Write};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex};

pub(super) fn get(command: &crate::workspace::CommandContext, stored_name: &str) -> Option<String> {
    crate::env::get(command, stored_name)
}

pub(super) struct CurlRequest {
    config: String,
}

pub(super) struct LimitedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
}

#[derive(Clone, Default)]
pub(crate) struct CurlCancellation {
    state: Arc<Mutex<CurlCancellationState>>,
}

#[derive(Default)]
struct CurlCancellationState {
    cancelled: bool,
    active_process_group: Option<i32>,
}

impl CurlCancellation {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn cancel(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.cancelled = true;
        if let Some(pid) = state.active_process_group {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.state.lock().map_or(true, |state| state.cancelled)
    }

    #[cfg(test)]
    pub(crate) fn is_cancelled_for_test(&self) -> bool {
        self.is_cancelled()
    }

    fn spawn_with_hook(
        &self,
        mut command: Command,
        after_spawn: impl FnOnce(u32),
    ) -> std::io::Result<std::process::Child> {
        use std::os::unix::process::CommandExt as _;

        if self.lock_state()?.cancelled {
            return Err(interrupted());
        }
        command.process_group(0);
        let mut child = command.spawn()?;
        after_spawn(child.id());
        let pid = i32::try_from(child.id())
            .map_err(|_| std::io::Error::other("curl process ID is outside platform range"))?;
        let mut state = self.lock_state()?;
        if state.cancelled {
            drop(state);
            terminate_and_reap(&mut child);
            return Err(interrupted());
        }
        state.active_process_group = Some(pid);
        drop(state);
        Ok(child)
    }

    fn finish(&self, pid: u32) {
        let Ok(pid) = i32::try_from(pid) else {
            return;
        };
        if let Ok(mut state) = self.state.lock()
            && state.active_process_group == Some(pid)
        {
            state.active_process_group = None;
        }
    }

    fn lock_state(&self) -> std::io::Result<std::sync::MutexGuard<'_, CurlCancellationState>> {
        self.state
            .lock()
            .map_err(|_| std::io::Error::other("curl cancellation state is unavailable"))
    }

    #[cfg(test)]
    pub(crate) fn run_for_test(
        &self,
        command: Command,
        after_spawn: impl FnOnce(u32),
    ) -> std::io::Result<ExitStatus> {
        let mut child = self.spawn_with_hook(command, after_spawn)?;
        let pid = child.id();
        let status = child.wait();
        self.finish(pid);
        if self.lock_state()?.cancelled {
            return Err(interrupted());
        }
        status
    }
}

fn interrupted() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "curl request was cancelled",
    )
}

fn terminate_and_reap(child: &mut std::process::Child) {
    let killed = i32::try_from(child.id()).is_ok_and(|pid| {
        nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        )
        .is_ok()
    });
    if !killed {
        let _ = child.kill();
    }
    let _ = child.wait();
}

impl CurlRequest {
    pub(super) fn new() -> Self {
        Self {
            config: String::new(),
        }
    }

    #[must_use]
    pub(super) fn flag(mut self, name: &str) -> Self {
        self.config.push_str(name);
        self.config.push('\n');
        self
    }

    #[must_use]
    pub(super) fn option(mut self, name: &str, value: &str) -> Self {
        self.config.push_str(name);
        self.config.push_str(" = \"");
        for character in value.chars() {
            match character {
                '\\' => self.config.push_str("\\\\"),
                '"' => self.config.push_str("\\\""),
                '\n' => self.config.push_str("\\n"),
                '\r' => self.config.push_str("\\r"),
                '\t' => self.config.push_str("\\t"),
                _ => self.config.push(character),
            }
        }
        self.config.push_str("\"\n");
        self
    }

    fn command() -> Command {
        let mut command = Command::new("curl");
        command
            .args(["--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    pub(super) fn output_cancellable(
        self,
        cancellation: &CurlCancellation,
    ) -> std::io::Result<Output> {
        let mut child = cancellation.spawn_with_hook(Self::command(), |_| {})?;
        let pid = child.id();
        let write_result = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("curl stdin was not piped"))
            .and_then(|mut stdin| stdin.write_all(self.config.as_bytes()));
        if let Err(error) = write_result {
            terminate_and_reap(&mut child);
            cancellation.finish(pid);
            return Err(error);
        }
        let output = child.wait_with_output();
        cancellation.finish(pid);
        output
    }

    pub(super) fn output_limited_cancellable(
        self,
        limit: usize,
        cancellation: &CurlCancellation,
    ) -> std::io::Result<LimitedOutput> {
        let mut command = Self::command();
        command.stderr(Stdio::null());
        let mut child = cancellation.spawn_with_hook(command, |_| {})?;
        let pid = child.id();
        let write_result = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("curl stdin was not piped"))
            .and_then(|mut stdin| stdin.write_all(self.config.as_bytes()));
        if let Err(error) = write_result {
            terminate_and_reap(&mut child);
            cancellation.finish(pid);
            return Err(error);
        }
        let read_result = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("curl stdout was not piped"))
            .and_then(|pipe| read_limited(pipe, limit));
        let stdout = match read_result {
            Ok(stdout) => stdout,
            Err(error) => {
                terminate_and_reap(&mut child);
                cancellation.finish(pid);
                return Err(error);
            }
        };
        let status = child.wait()?;
        cancellation.finish(pid);
        Ok(LimitedOutput { status, stdout })
    }

    #[cfg(test)]
    pub(super) fn has_exact_option_for_test(&self, name: &str, value: &str) -> bool {
        let exact = Self::new().option(name, value).config;
        self.config
            .lines()
            .any(|line| exact.strip_suffix('\n') == Some(line))
    }

    #[cfg(test)]
    pub(super) fn option_prefix_count_for_test(&self, name: &str, prefix: &str) -> usize {
        let exact = Self::new().option(name, prefix).config;
        let stem = exact.strip_suffix("\"\n").unwrap_or(&exact);
        self.config.matches(stem).count()
    }

    #[cfg(test)]
    pub(super) fn redacted_digest_for_test(&self) -> [u8; 32] {
        use sha2::Digest as _;

        sha2::Sha256::digest(self.config.as_bytes()).into()
    }
}

fn read_limited(reader: impl std::io::Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let proof_limit = limit
        .checked_add(1)
        .ok_or_else(|| std::io::Error::other("curl response limit overflow"))?;
    let mut output = Vec::with_capacity(limit.min(16 * 1024));
    reader
        .take(u64::try_from(proof_limit).unwrap_or(u64::MAX))
        .read_to_end(&mut output)?;
    if output.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "curl response exceeds configured limit",
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use serde_json::{Map, json};

    use super::{CurlCancellation, CurlRequest, get, read_limited};
    use crate::workspace::{
        CommandContext, MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId,
        WorkspaceName, WorkspaceRecord,
    };

    const PROVIDER_ENV_CHILD: &str = "BRAIN_PROVIDER_ENV_ISOLATION_CHILD";

    #[test]
    fn cancelled_provider_work_refuses_to_start_a_subprocess() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let marker = temporary.path().join("provider-ran");
        let cancellation = CurlCancellation::new();
        cancellation.cancel();
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", &format!("touch {}", marker.display())]);

        let error = cancellation
            .run_for_test(command, |_| {})
            .expect_err("cancelled work must not spawn");

        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        assert!(!marker.exists());
    }

    #[test]
    fn cancellation_between_spawn_and_publish_terminates_and_reaps_the_process_group() {
        let cancellation = CurlCancellation::new();
        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", "read _"])
            .stdin(std::process::Stdio::piped());
        let mut published_pid = None;

        let error = cancellation
            .run_for_test(command, |pid| {
                published_pid = Some(pid);
                cancellation.cancel();
            })
            .expect_err("cancel during publication must interrupt the child");

        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        let pid = i32::try_from(published_pid.expect("spawned process ID")).expect("PID range");
        assert_eq!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH),
            "cancelled provider child must be reaped before returning"
        );
    }

    fn workspace_record(
        workspace_id: &str,
        root: &std::path::Path,
        provider_value: Option<&str>,
    ) -> WorkspaceRecord {
        let mut env = Map::new();
        if let Some(value) = provider_value {
            env.insert("twilio_auth_token".to_owned(), json!(value));
        }
        WorkspaceRecord {
            workspace_id: WorkspaceId::parse(workspace_id).expect("valid workspace id"),
            root: root.to_owned(),
            aliases: BTreeSet::new(),
            local_user_id: "pablo".to_owned(),
            receiver_enabled: true,
            env,
        }
    }

    fn command_context(
        home: &std::path::Path,
        store: &RegistryStore,
        name: &str,
        record: &WorkspaceRecord,
    ) -> CommandContext {
        CommandContext::new(
            Arc::new(
                WorkspaceContext::new(
                    home,
                    record.workspace_id,
                    WorkspaceName::parse(name).expect("valid workspace name"),
                    &record.root,
                    &record.local_user_id,
                    home,
                )
                .expect("workspace context"),
            ),
            store.clone(),
        )
        .unwrap()
    }

    #[test]
    fn selected_workspace_provider_values_ignore_the_process_environment() {
        if std::env::var_os(PROVIDER_ENV_CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "server::provider::tests::selected_workspace_provider_values_ignore_the_process_environment",
                    "--nocapture",
                ])
                .env(PROVIDER_ENV_CHILD, "1")
                .env("TWILIO_AUTH_TOKEN", "process-token")
                .output()
                .expect("child test process");

            assert!(
                output.status.success(),
                "child failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let home = tempfile::tempdir().expect("temporary home");
        let personal_name = WorkspaceName::parse("personal").expect("valid name");
        let family_name = WorkspaceName::parse("family").expect("valid name");
        let personal = workspace_record(
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            &home.path().join("personal"),
            Some("personal-token"),
        );
        let family = workspace_record(
            "e806258e-491a-436d-9db4-a5ca9903e0d4",
            &home.path().join("family"),
            None,
        );
        let registry = MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (personal_name, personal.clone()),
                (family_name, family.clone()),
            ]),
            env: serde_json::Map::new(),
        };
        let store = RegistryStore::from_path(home.path().join("config/brain/env.json"));
        store.replace(&registry).expect("write registry");
        let personal_command = command_context(home.path(), &store, "personal", &personal);
        let family_command = command_context(home.path(), &store, "family", &family);

        assert_eq!(
            get(&personal_command, "twilio_auth_token"),
            Some("personal-token".to_owned())
        );
        assert_eq!(get(&family_command, "twilio_auth_token"), None);
    }

    #[test]
    fn curl_request_keeps_secrets_and_content_out_of_process_arguments() {
        let secret = "secret-token";
        let content = "private message";
        let request = CurlRequest::new()
            .option("header", &format!("Authorization: Bearer {secret}"))
            .option("data", content);

        let command = CurlRequest::command();
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy())
            .collect::<Vec<_>>();

        assert!(
            request.has_exact_option_for_test("header", &format!("Authorization: Bearer {secret}"))
        );
        assert!(request.has_exact_option_for_test("data", content));
        assert_eq!(arguments, ["--config", "-"]);
        assert!(!arguments.iter().any(|argument| argument.contains(secret)));
        assert!(!arguments.iter().any(|argument| argument.contains(content)));
    }

    #[test]
    fn curl_config_quotes_control_characters_and_double_quotes() {
        let private_control_value = "line 1\n\"line 2\"\\end";
        let request = CurlRequest::new().option("data", private_control_value);

        assert!(request.has_exact_option_for_test("data", private_control_value));
    }

    #[test]
    fn limited_output_reads_only_one_proof_byte_past_the_cap() {
        let payload = vec![b'x'; 64];
        let mut reader = CountingReader::new(&payload);

        let error = read_limited(&mut reader, 8).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(reader.consumed, 9);
    }

    struct CountingReader<'a> {
        remaining: &'a [u8],
        consumed: usize,
    }

    impl<'a> CountingReader<'a> {
        const fn new(bytes: &'a [u8]) -> Self {
            Self {
                remaining: bytes,
                consumed: 0,
            }
        }
    }

    impl std::io::Read for CountingReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = buffer.len().min(self.remaining.len());
            buffer[..count].copy_from_slice(&self.remaining[..count]);
            self.remaining = &self.remaining[count..];
            self.consumed += count;
            Ok(count)
        }
    }
}
