//! Machine-local provider configuration with process-environment compatibility.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn prefer_process_value(process: Option<String>, stored: Option<String>) -> Option<String> {
    process.filter(|value| !value.trim().is_empty()).or(stored)
}

pub(super) fn get(process_name: &str, stored_name: &str) -> Option<String> {
    prefer_process_value(
        std::env::var(process_name).ok(),
        crate::env::get(stored_name),
    )
}

pub(super) struct CurlRequest {
    config: String,
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

    #[cfg(test)]
    fn config(&self) -> &str {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::{CurlRequest, prefer_process_value};

    #[test]
    fn nonempty_process_value_overrides_machine_local_value() {
        assert_eq!(
            prefer_process_value(Some("process".to_owned()), Some("stored".to_owned())),
            Some("process".to_owned())
        );
    }

    #[test]
    fn blank_process_value_falls_back_to_machine_local_value() {
        assert_eq!(
            prefer_process_value(Some("  ".to_owned()), Some("stored".to_owned())),
            Some("stored".to_owned())
        );
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
}
