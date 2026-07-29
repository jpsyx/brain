//! TUI-owned messaging server.
//!
//! This listener is intentionally a child of the interactive brain process.
//! Dropping [`MessagingServer`] closes the socket, so it cannot become a
//! detached service on machines that are not meant to receive messages.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, mpsc::Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use tiny_http::{Header, Method, Response, Server};

use super::router::Route;

#[derive(Debug, Clone)]
struct SecurityConfig {
    twilio_auth_token: String,
    public_url: String,
    resend_signing_secret: String,
    allowed_sms: Vec<String>,
    allowed_email: Vec<String>,
}

impl SecurityConfig {
    fn load() -> Self {
        let config = crate::config::Config::load();
        Self {
            twilio_auth_token: std::env::var("TWILIO_AUTH_TOKEN").unwrap_or_default(),
            public_url: std::env::var("BRAIN_MESSAGING_PUBLIC_URL").unwrap_or_default(),
            resend_signing_secret: std::env::var("RESEND_WEBHOOK_SIGNING_SECRET")
                .unwrap_or_default(),
            allowed_sms: config.allowed_sms(),
            allowed_email: config.allowed_email(),
        }
    }
}

pub const DEFAULT_PORT: u16 = 8788;

#[must_use]
pub fn control_path() -> PathBuf {
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".cache/brain/messaging.sock"),
        |home| PathBuf::from(home).join(".cache/brain/messaging.sock"),
    )
}

pub struct ControlSocket {
    listener: UnixListener,
    path: PathBuf,
}

impl ControlSocket {
    pub fn bind() -> Result<Self> {
        let path = control_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let _ = std::fs::remove_file(&path);
        let listener =
            UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
        listener
            .set_nonblocking(true)
            .context("making messaging control socket nonblocking")?;
        Ok(Self { listener, path })
    }

    fn drain(&self) -> Vec<(UnixStream, String)> {
        let mut requests = Vec::new();
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let mut command = String::new();
                    let _ = stream.read_to_string(&mut command);
                    requests.push((stream, command.trim().to_owned()));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        requests
    }

    #[must_use]
    pub fn poll(&self) -> Vec<(UnixStream, String)> {
        self.drain()
    }
}

impl Drop for ControlSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn send_control(command: &str) -> Result<String> {
    let mut stream =
        UnixStream::connect(control_path()).context("connecting to the running brain TUI")?;
    stream
        .write_all(command.as_bytes())
        .context("sending messaging command")?;
    stream.shutdown(std::net::Shutdown::Write).ok();
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("reading messaging command response")?;
    Ok(response)
}

#[cfg(test)]
mod attachment_tests {
    use super::safe_attachment_name;

    #[test]
    fn attachment_names_cannot_escape_the_job_directory() {
        assert_eq!(
            safe_attachment_name("https://example.test/../../paper.pdf?x=1", 0),
            "0-paper.pdf"
        );
        assert_eq!(
            safe_attachment_name("https://example.test/", 1),
            "attachment-1"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Sms,
    Email,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    pub channel: Channel,
    pub body: String,
    pub sender: String,
    pub provider_id: Option<String>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub url: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedAttachment {
    pub source: String,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
}

/// Download every inbound media item into a job-scoped cache directory. A
/// failed download is returned as data so the agent can report it explicitly.
#[must_use]
pub fn stage_attachments(message: &InboundMessage) -> Vec<StagedAttachment> {
    let job = uuid::Uuid::new_v4().to_string();
    let dir = std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".cache/brain/inbox").join(&job),
        |home| PathBuf::from(home).join(".cache/brain/inbox").join(&job),
    );
    let _ = std::fs::create_dir_all(&dir);
    message
        .attachments
        .iter()
        .enumerate()
        .map(|(index, attachment)| {
            let name = safe_attachment_name(&attachment.url, index);
            let path = dir.join(name);
            let mut command = std::process::Command::new("curl");
            command.args(["-fsSL", "--max-time", "60", "--max-filesize", "41943040"]);
            match message.channel {
                Channel::Sms => {
                    if let (Ok(account), Ok(token)) = (
                        std::env::var("TWILIO_ACCOUNT_SID"),
                        std::env::var("TWILIO_AUTH_TOKEN"),
                    ) {
                        command.args(["-u", &format!("{account}:{token}")]);
                    }
                }
                Channel::Email => {
                    if let Ok(key) = std::env::var("RESEND_API_KEY") {
                        command.args(["-H", &format!("Authorization: Bearer {key}")]);
                    }
                }
            }
            command.args(["-o", path.to_string_lossy().as_ref(), &attachment.url]);
            match command.status() {
                Ok(status) if status.success() => StagedAttachment {
                    source: attachment.url.clone(),
                    path: Some(path),
                    error: None,
                },
                Ok(status) => StagedAttachment {
                    source: attachment.url.clone(),
                    path: None,
                    error: Some(format!("download exited with {status}")),
                },
                Err(error) => StagedAttachment {
                    source: attachment.url.clone(),
                    path: None,
                    error: Some(error.to_string()),
                },
            }
        })
        .collect()
}

fn safe_attachment_name(url: &str, index: usize) -> String {
    let raw = url.rsplit('/').next().unwrap_or_default();
    let stem = raw.split('?').next().unwrap_or_default();
    let clean: String = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        format!("attachment-{index}")
    } else {
        format!("{index}-{clean}")
    }
}

pub struct MessagingServer {
    server: Arc<Server>,
    join: Option<JoinHandle<()>>,
}

impl MessagingServer {
    /// Bind and start the TUI-owned listener.
    pub fn start(port: u16, tx: Sender<InboundMessage>) -> Result<Self> {
        let server = Arc::new(
            Server::http(("127.0.0.1", port))
                .map_err(|e| anyhow::anyhow!("binding messaging server: {e}"))?,
        );
        let actual = server
            .server_addr()
            .to_ip()
            .context("resolving messaging server address")?
            .port();
        let worker_server = Arc::clone(&server);
        let security = SecurityConfig::load();
        let join = thread::Builder::new()
            .name("brain-messaging-server".to_owned())
            .spawn(move || {
                while let Ok(Some(mut request)) =
                    worker_server.recv_timeout(Duration::from_millis(100))
                {
                    let response = respond(&mut request, &tx, &security);
                    let _ = request.respond(response);
                }
            })
            .context("starting messaging server thread")?;
        let _ = actual;
        Ok(Self {
            server,
            join: Some(join),
        })
    }
}

impl Drop for MessagingServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn respond(
    request: &mut tiny_http::Request,
    tx: &Sender<InboundMessage>,
    security: &SecurityConfig,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let route = super::router::route(request.method().as_str(), request.url());
    let mut body = Vec::new();
    let _ = request.as_reader().read_to_end(&mut body);
    let result = match route {
        Route::Sms => parse_sms(request, &body, security).and_then(|message| enqueue(tx, message)),
        Route::Email => {
            parse_email(request, &body, security).and_then(|message| enqueue(tx, message))
        }
        _ => Err((404, "not found".to_owned())),
    };
    match result {
        Ok(()) if route == Route::Sms => Response::from_string(
            "<Response><Message>Received. I’ll get back to you shortly.</Message></Response>",
        )
        .with_status_code(200)
        .with_header(xml_header()),
        Ok(()) => Response::from_string("{\"ok\":true,\"queued\":true}")
            .with_status_code(202)
            .with_header(json_header()),
        Err((status, error)) => Response::from_string(format!(
            "{{\"ok\":false,\"error\":{}}}",
            serde_json::to_string(&error).unwrap_or_else(|_| "\"request rejected\"".to_owned())
        ))
        .with_status_code(status)
        .with_header(json_header()),
    }
}

fn enqueue(tx: &Sender<InboundMessage>, message: InboundMessage) -> Result<(), (u16, String)> {
    tx.send(message)
        .map_err(|_| (503, "brain is not accepting messages".to_owned()))
}

fn parse_sms(
    request: &tiny_http::Request,
    body: &[u8],
    security: &SecurityConfig,
) -> Result<InboundMessage, (u16, String)> {
    if request.method() != &Method::Post {
        return Err((405, "method not allowed".to_owned()));
    }
    let fields = parse_form(body)?;
    if security.twilio_auth_token.is_empty() || security.public_url.is_empty() {
        return Err((503, "Twilio security is not configured".to_owned()));
    }
    let sorted = fields
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let signature = request
        .headers()
        .iter()
        .find(|header| {
            header
                .field
                .to_string()
                .eq_ignore_ascii_case("X-Twilio-Signature")
        })
        .map(|header| header.value.as_str())
        .unwrap_or_default();
    if !crate::server::security::verify_twilio(
        &security.twilio_auth_token,
        &security.public_url,
        &sorted,
        signature,
    ) {
        return Err((403, "invalid Twilio signature".to_owned()));
    }
    let text = fields.get("Body").cloned().unwrap_or_default();
    let sender = fields.get("From").cloned().unwrap_or_default();
    if !crate::server::security::sender_allowed(&sender, &security.allowed_sms) {
        return Err((403, "SMS sender is not allowed".to_owned()));
    }
    if text.trim().is_empty() && !fields.keys().any(|key| key.starts_with("MediaUrl")) {
        return Err((400, "SMS body and media are both empty".to_owned()));
    }
    Ok(InboundMessage {
        channel: Channel::Sms,
        body: text,
        sender,
        provider_id: fields.get("MessageSid").cloned(),
        attachments: fields
            .iter()
            .filter(|(key, _)| key.starts_with("MediaUrl"))
            .map(|(_, url)| Attachment {
                url: url.clone(),
                content_type: None,
            })
            .collect(),
    })
}

#[derive(Deserialize)]
struct ResendWebhook {
    #[serde(rename = "type")]
    event_type: String,
    data: ResendData,
}

#[derive(Deserialize)]
struct ResendData {
    #[serde(default)]
    from: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    email_id: Option<String>,
    #[serde(default)]
    attachments: Vec<ResendAttachment>,
}

#[derive(Deserialize)]
struct ResendAttachment {
    #[serde(default)]
    url: String,
    #[serde(default)]
    content_type: Option<String>,
}

fn parse_email(
    request: &tiny_http::Request,
    body: &[u8],
    security: &SecurityConfig,
) -> Result<InboundMessage, (u16, String)> {
    if security.resend_signing_secret.is_empty() {
        return Err((503, "Resend security is not configured".to_owned()));
    }
    let header = |name: &str| {
        request
            .headers()
            .iter()
            .find(|header| header.field.to_string().eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
            .unwrap_or_default()
    };
    let webhook_id = header("svix-id");
    let timestamp = header("svix-timestamp");
    if !crate::server::security::verify_resend(
        &security.resend_signing_secret,
        webhook_id,
        timestamp,
        body,
        header("svix-signature"),
    ) {
        return Err((403, "invalid Resend signature".to_owned()));
    }
    let mut webhook: ResendWebhook = serde_json::from_slice(body)
        .map_err(|_| (400, "invalid Resend webhook JSON".to_owned()))?;
    if webhook.event_type != "email.received" {
        return Err((202, "event ignored".to_owned()));
    }
    if webhook.data.text.trim().is_empty() {
        let Some(email_id) = webhook.data.email_id.as_deref() else {
            return Err((400, "received email has no body or email id".to_owned()));
        };
        let (text, attachments) = fetch_resend_email(email_id)?;
        webhook.data.text = text;
        webhook.data.attachments = attachments;
    }
    if webhook.data.text.trim().is_empty() && webhook.data.attachments.is_empty() {
        return Err((
            400,
            "received email has no text body or attachment".to_owned(),
        ));
    }
    if !crate::server::security::sender_allowed(&webhook.data.from, &security.allowed_email) {
        return Err((403, "email sender is not allowed".to_owned()));
    }
    Ok(InboundMessage {
        channel: Channel::Email,
        body: webhook.data.text,
        sender: webhook.data.from,
        provider_id: webhook.data.email_id,
        attachments: webhook
            .data
            .attachments
            .into_iter()
            .map(|attachment| Attachment {
                url: attachment.url,
                content_type: attachment.content_type,
            })
            .collect(),
    })
}

fn fetch_resend_email(email_id: &str) -> Result<(String, Vec<ResendAttachment>), (u16, String)> {
    let key = std::env::var("RESEND_API_KEY")
        .map_err(|_| (503, "RESEND_API_KEY is not configured".to_owned()))?;
    let url = format!("https://api.resend.com/emails/{email_id}");
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "30",
            "-H",
            &format!("Authorization: Bearer {key}"),
            &url,
        ])
        .output()
        .map_err(|error| (502, format!("fetching received email: {error}")))?;
    if !output.status.success() {
        return Err((
            502,
            "Resend receiving API rejected the email fetch".to_owned(),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| (502, "Resend returned invalid email content".to_owned()))?;
    let text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let attachments = value
        .get("attachments")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let url = item
                        .get("download_url")
                        .or_else(|| item.get("url"))?
                        .as_str()?
                        .to_owned();
                    Some(ResendAttachment {
                        url,
                        content_type: item
                            .get("content_type")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((text, attachments))
}

fn parse_form(body: &[u8]) -> Result<std::collections::HashMap<String, String>, (u16, String)> {
    let text = std::str::from_utf8(body).map_err(|_| (400, "SMS body is not UTF-8".to_owned()))?;
    Ok(text
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((decode_form(key), decode_form(value)))
        })
        .collect())
}

fn decode_form(value: &str) -> String {
    let value = value.replace('+', " ");
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(hex);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn json_header() -> Header {
    Header::from_bytes(&b"Content-Type"[..], b"application/json")
        .unwrap_or_else(|()| unreachable!("static header is valid"))
}

fn xml_header() -> Header {
    Header::from_bytes(&b"Content-Type"[..], b"application/xml")
        .unwrap_or_else(|()| unreachable!("static header is valid"))
}
