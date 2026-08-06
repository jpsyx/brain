//! Workspace-siloed machine-local provider configuration.

use std::io::{Read as _, Write};
use std::process::{Command, ExitStatus, Output, Stdio};

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

    pub(super) fn output(self) -> std::io::Result<Output> {
        let mut child = Self::command().spawn()?;
        let write_result = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("curl stdin was not piped"))
            .and_then(|mut stdin| stdin.write_all(self.config.as_bytes()));
        if let Err(error) = write_result {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        child.wait_with_output()
    }

    pub(super) fn output_limited(self, limit: usize) -> std::io::Result<LimitedOutput> {
        let mut command = Self::command();
        command.stderr(Stdio::null());
        let mut child = command.spawn()?;
        let write_result = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("curl stdin was not piped"))
            .and_then(|mut stdin| stdin.write_all(self.config.as_bytes()));
        if let Err(error) = write_result {
            let _ = child.kill();
            let _ = child.wait();
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
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let status = child.wait()?;
        Ok(LimitedOutput { status, stdout })
    }

    #[cfg(test)]
    fn config(&self) -> &str {
        &self.config
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

    use super::{CurlRequest, get, read_limited};
    use crate::workspace::{
        CommandContext, MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId,
        WorkspaceName, WorkspaceRecord,
    };

    const PROVIDER_ENV_CHILD: &str = "BRAIN_PROVIDER_ENV_ISOLATION_CHILD";

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
            schema_version: 2,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (personal_name, personal.clone()),
                (family_name, family.clone()),
            ]),
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

        assert!(request.config().contains(secret));
        assert!(request.config().contains(content));
        assert_eq!(arguments, ["--config", "-"]);
        assert!(!arguments.iter().any(|argument| argument.contains(secret)));
        assert!(!arguments.iter().any(|argument| argument.contains(content)));
    }

    #[test]
    fn curl_config_quotes_control_characters_and_double_quotes() {
        let request = CurlRequest::new().option("data", "line 1\n\"line 2\"\\end");

        assert!(
            request
                .config()
                .contains("data = \"line 1\\n\\\"line 2\\\"\\\\end\"")
        );
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
