//! Per-run logging, with optional stdout mirroring.

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::{Local, SecondsFormat};

mod receiver_lifecycle;

pub(crate) use receiver_lifecycle::{
    ReceiverDeliveryPhase, ReceiverLifecycleEvent, ReceiverLifecyclePhase, ReceiverLifecycleReason,
};

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static STDOUT_ENABLED: AtomicBool = AtomicBool::new(false);

/// Owns this run's verbose logger lifecycle.
pub struct Guard {
    path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        log("brain end");
        if stdout_enabled() {
            println!("verbose log: {}", self.path.display());
        }
    }
}

/// File-backed verbose logger.
pub(crate) struct Logger {
    file: File,
}

impl Logger {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        options.open(path).map(|file| Self { file })
    }

    pub(crate) fn write(&mut self, message: &str) -> io::Result<()> {
        writeln!(self.file, "{message}")?;
        self.file.flush()
    }
}

pub fn init(verbose: bool, stdout: bool) -> io::Result<Guard> {
    STDOUT_ENABLED.store(verbose && stdout, Ordering::Relaxed);
    let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Nanos, false);
    let path = log_path_for(&timestamp);
    let logger = Logger::open(&path)?;
    let _ = LOG_PATH.set(path.clone());
    let _ = LOGGER.set(Mutex::new(logger));
    log(format!("brain start {}", env!("CARGO_PKG_VERSION")));
    Ok(Guard { path })
}

pub fn log(message: impl AsRef<str>) {
    let message = message.as_ref();
    let line = format!(
        "{} {message}",
        Local::now().to_rfc3339_opts(SecondsFormat::Nanos, false)
    );
    if let Some(logger) = LOGGER.get() {
        let _ = logger.lock().map(|mut logger| logger.write(&line));
    }
    if stdout_enabled() {
        println!("{line}");
    }
}

/// Emit one stable content-free receiver transition record.
pub(crate) fn log_receiver_lifecycle(event: ReceiverLifecycleEvent) {
    let line = event.to_string();
    #[cfg(test)]
    RECEIVER_LIFECYCLE_CAPTURE.with(|capture| {
        if let Some(records) = capture.borrow_mut().as_mut() {
            records.push(line.clone());
        }
    });
    #[cfg(not(test))]
    crate::server::lifecycle::append_event_log(&line);
    log(line);
}

#[cfg(test)]
thread_local! {
    static RECEIVER_LIFECYCLE_CAPTURE: std::cell::RefCell<Option<Vec<String>>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
pub(crate) fn capture_receiver_lifecycle(operation: impl FnOnce()) -> Vec<String> {
    RECEIVER_LIFECYCLE_CAPTURE.with(|capture| {
        assert!(
            capture.borrow().is_none(),
            "nested receiver lifecycle capture"
        );
        *capture.borrow_mut() = Some(Vec::new());
    });
    operation();
    RECEIVER_LIFECYCLE_CAPTURE.with(|capture| {
        capture
            .borrow_mut()
            .take()
            .expect("receiver lifecycle capture was active")
    })
}

/// Redact private command-line values before they cross the logging boundary.
#[must_use]
pub fn redact_argv(argv: &[String]) -> Vec<String> {
    let mut redact_next = false;
    argv.iter()
        .map(|argument| {
            if redact_next {
                redact_next = false;
                return "[REDACTED]".to_owned();
            }
            if let Some((flag, _)) = argument.split_once('=')
                && is_private_setup_flag(flag)
            {
                return format!("{flag}=[REDACTED]");
            }
            if is_private_setup_flag(argument) {
                redact_next = true;
                return argument.clone();
            }
            if let Some((name, _)) = argument.split_once('=')
                && {
                    let canonical = crate::settings::normalize_name(name);
                    is_private_receiver_field(&canonical) || crate::env::is_sensitive(&canonical)
                }
            {
                return format!("{name}=[REDACTED]");
            }
            argument.clone()
        })
        .collect()
}

fn is_private_setup_flag(value: &str) -> bool {
    matches!(
        value,
        "--public-url"
            | "--twilio-account-sid"
            | "--twilio-auth-token"
            | "--twilio-from-number"
            | "--resend-sending-api-key"
            | "--resend-from-email"
            | "--resend-webhook-signing-secret"
            | "--phone"
            | "--email"
            | "--response-email"
            | "--user-name"
    )
}

fn is_private_receiver_field(value: &str) -> bool {
    matches!(
        value,
        "brain_receiver_public_url"
            | "twilio_account_sid"
            | "twilio_auth_token"
            | "twilio_from_number"
            | "resend_sending_api_key"
            | "resend_full_access_api_key"
            | "resend_from_email"
            | "resend_webhook_signing_secret"
    )
}

pub fn path() -> Option<PathBuf> {
    LOG_PATH.get().cloned()
}

pub fn set_stdout_enabled(enabled: bool) {
    STDOUT_ENABLED.store(enabled, Ordering::Relaxed);
}

fn stdout_enabled() -> bool {
    STDOUT_ENABLED.load(Ordering::Relaxed)
}

#[must_use]
pub(crate) fn log_path_for(timestamp: &str) -> PathBuf {
    PathBuf::from("/tmp").join(format!("{timestamp}-{}.log", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    #[test]
    fn receiver_lifecycle_records_have_stable_content_free_fields() {
        use super::{
            ReceiverDeliveryPhase, ReceiverLifecycleEvent, ReceiverLifecyclePhase,
            ReceiverLifecycleReason,
        };

        let cases = [
            (
                ReceiverLifecycleEvent::ingress(4),
                "receiver lifecycle event=ingress phase=queued queue_depth=4",
            ),
            (
                ReceiverLifecycleEvent::claim(3),
                "receiver lifecycle event=claim phase=claimed queue_depth=3",
            ),
            (
                ReceiverLifecycleEvent::launch(0, 3),
                "receiver lifecycle event=launch phase=launched recovery=0/3",
            ),
            (
                ReceiverLifecycleEvent::observation(ReceiverLifecyclePhase::Accepted),
                "receiver lifecycle event=acceptance phase=accepted",
            ),
            (
                ReceiverLifecycleEvent::observation(ReceiverLifecyclePhase::Processing),
                "receiver lifecycle event=progress phase=processing",
            ),
            (
                ReceiverLifecycleEvent::recovery(
                    ReceiverLifecyclePhase::Retrying,
                    2,
                    3,
                    ReceiverLifecycleReason::AcceptedStall,
                ),
                "receiver lifecycle event=recovery phase=retrying recovery=2/3 reason=accepted-stall",
            ),
            (
                ReceiverLifecycleEvent::answer_ready(1),
                "receiver lifecycle event=answer-readiness phase=answer-ready cleanup_gated=1",
            ),
            (
                ReceiverLifecycleEvent::cleanup_promotion(0),
                "receiver lifecycle event=cleanup-promotion delivery_phase=ready cleanup_gated=0",
            ),
            (
                ReceiverLifecycleEvent::delivery_result(
                    ReceiverDeliveryPhase::Retrying,
                    ReceiverLifecycleReason::TransportUnavailable,
                ),
                "receiver lifecycle event=delivery-result delivery_phase=retrying reason=transport-unavailable",
            ),
            (
                ReceiverLifecycleEvent::terminal(
                    ReceiverLifecyclePhase::Done,
                    0,
                    ReceiverLifecycleReason::ProviderAcknowledged,
                ),
                "receiver lifecycle event=terminal-advancement phase=done queue_depth=0 reason=provider-acknowledged",
            ),
        ];

        for (event, expected) in cases {
            assert_eq!(event.to_string(), expected);
        }
    }

    #[test]
    fn receiver_lifecycle_types_cannot_carry_private_identity_or_content() {
        let rendered = format!(
            "{:?}",
            super::ReceiverLifecycleEvent::delivery_result(
                super::ReceiverDeliveryPhase::Acknowledged,
                super::ReceiverLifecycleReason::ProviderAcknowledged,
            )
        );

        for private in [
            "private-workspace",
            "private-actor",
            "private-job-id",
            "private-prompt",
            "private-answer",
            "/private/state.db",
        ] {
            assert!(!rendered.contains(private));
        }
    }

    #[test]
    fn log_path_uses_tmp_and_the_exact_timestamp_as_filename() {
        let path = log_path_for("2026-07-26T10:11:12.123456789-04:00");
        assert!(
            path.to_string_lossy()
                .starts_with("/tmp/2026-07-26T10:11:12.123456789-04:00-")
        );
        assert!(path.extension().is_some_and(|ext| ext == "log"));
    }

    #[test]
    fn logger_writes_lines_to_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("run.log");
        let mut logger = Logger::open(&path).unwrap();

        logger.write("hello world").unwrap();

        let mut text = String::new();
        std::fs::File::open(path)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.contains("hello world"), "{text}");
    }

    #[test]
    fn argv_redaction_covers_separate_and_assignment_style_private_values() {
        let argv = [
            "brain",
            "receiver",
            "setup",
            "--public-url=https://private-origin.example.test",
            "--twilio-auth-token",
            "token-separate",
            "--phone=+12125550100",
            "--email",
            "private@example.test",
            "--response-email=response@example.test",
        ]
        .map(str::to_owned);

        let redacted = redact_argv(&argv);

        let text = format!("{redacted:?}");
        for private in [
            "private-origin.example.test",
            "token-separate",
            "+12125550100",
            "private@example.test",
            "response@example.test",
        ] {
            assert!(!text.contains(private), "leaked {private}: {text}");
        }
        assert_eq!(redacted[3], "--public-url=[REDACTED]");
        assert_eq!(redacted[5], "[REDACTED]");
        assert_eq!(redacted[6], "--phone=[REDACTED]");
    }

    #[test]
    fn argv_redaction_covers_receiver_set_assignments_without_hiding_safe_values() {
        let argv = [
            "brain",
            "-w",
            "family",
            "receiver",
            "set",
            "twilio_auth_token=set-secret",
            "--verbose",
        ]
        .map(str::to_owned);

        assert_eq!(
            redact_argv(&argv),
            [
                "brain",
                "-w",
                "family",
                "receiver",
                "set",
                "twilio_auth_token=[REDACTED]",
                "--verbose",
            ]
        );
    }

    #[test]
    fn argv_logging_boundary_uses_env_sensitivity_for_nested_agent_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("argv.log");
        let mut logger = Logger::open(&path).unwrap();
        let argv = [
            "brain",
            "env",
            "set",
            r#"agent_capabilities={"mcps":{"search":{"credentials":{"token":"top-level-secret"}}}}"#,
            "agent_capabilities.mcps.search.credentials.bearer_token=nested-secret",
            "codex_cmd=codex --safe",
        ]
        .map(str::to_owned);

        logger
            .write(&format!("argv {:?}", redact_argv(&argv)))
            .unwrap();

        let contents = std::fs::read_to_string(path).unwrap();
        assert!(!contents.contains("top-level-secret"), "{contents}");
        assert!(!contents.contains("nested-secret"), "{contents}");
        assert!(contents.contains("codex_cmd=codex --safe"), "{contents}");
    }

    #[cfg(unix)]
    #[test]
    fn logger_creates_a_private_run_log() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("private.log");
        let _logger = Logger::open(&path).unwrap();

        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
