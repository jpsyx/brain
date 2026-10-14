//! Versioned adapter messages and strict response matching.
use serde::{Deserialize, Serialize};

pub(crate) const LIMIT: usize = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub schema_version: u32,
    pub operation_id: String,
    pub action_id: String,
    pub workspace_id: String,
    pub actor_id: String,
    pub action: String,
    pub issue: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Response {
    pub schema_version: u32,
    pub operation_id: String,
    pub action_id: String,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    Configuration,
    RequestTooLarge,
    ResponseTooLarge,
    StderrTooLarge,
    Spawn,
    Io,
    Timeout,
    Exit,
    Protocol,
}

/// Contains only fixed diagnostics and redacted, complete stderr capture.
#[derive(Debug)]
pub(crate) struct AdapterError {
    pub kind: ErrorKind,
    pub stderr: String,
}

impl AdapterError {
    pub(super) const fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            stderr: String::new(),
        }
    }
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "removal adapter {:?}", self.kind)?;
        if !self.stderr.is_empty() {
            write!(f, ": {}", self.stderr)?;
        }
        Ok(())
    }
}
impl std::error::Error for AdapterError {}

// The serializer cannot allocate an unbounded second copy of a large issue.
struct Bounded(Vec<u8>);
impl std::io::Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > LIMIT - self.0.len() {
            return Err(std::io::Error::other("request exceeds limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn encode(request: &Request) -> Result<Vec<u8>, AdapterError> {
    if request.schema_version != 1
        || !matches!(request.action.as_str(), "preflight" | "cancel" | "status")
    {
        return Err(AdapterError::new(ErrorKind::Protocol));
    }
    let mut writer = Bounded(Vec::new());
    serde_json::to_writer(&mut writer, request)
        .map_err(|_| AdapterError::new(ErrorKind::RequestTooLarge))?;
    Ok(writer.0)
}

pub(crate) fn decode(request: &Request, bytes: &[u8]) -> Result<Response, AdapterError> {
    if bytes.len() > LIMIT {
        return Err(AdapterError::new(ErrorKind::ResponseTooLarge));
    }
    let response: Response =
        serde_json::from_slice(bytes).map_err(|_| AdapterError::new(ErrorKind::Protocol))?;
    let valid_pair = matches!(
        (request.action.as_str(), response.status.as_str()),
        ("preflight", "ready" | "error")
            | ("cancel", "canceled" | "already_canceled" | "error")
            | (
                "status",
                "canceled" | "already_canceled" | "pending" | "error"
            )
    );
    if request.schema_version != 1
        || response.schema_version != 1
        || response.operation_id != request.operation_id
        || response.action_id != request.action_id
        || !valid_pair
    {
        return Err(AdapterError::new(ErrorKind::Protocol));
    }
    Ok(response)
}
