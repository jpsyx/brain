mod email;
mod sms;

use std::io::Read;
use std::sync::{
    Arc, Mutex,
    mpsc::{SyncSender, TrySendError},
};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use tiny_http::{Header, Response, Server};

use crate::server::receiver::{Channel, InboundMessage};
use crate::server::router::Route;

const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const RECENT_PROVIDER_IDS: usize = 1024;
const RECEIVER_WORKERS: usize = 4;

#[derive(Debug, Clone)]
pub(super) struct SecurityConfig {
    twilio_auth_token: String,
    public_base_url: String,
    resend_signing_secret: String,
    resend_api_key: String,
    workspace_id: crate::workspace::WorkspaceId,
    local_user_id: crate::users::UserId,
    users: crate::users::Users,
}

impl SecurityConfig {
    fn load(command: &crate::workspace::CommandContext) -> Result<Self> {
        let local_user_id = crate::users::UserId::parse(command.workspace.local_user_id())
            .context("parsing selected local user")?;
        let users = crate::users::UsersStore::load(&command.workspace)
            .context("loading portable workspace users")?;
        Ok(Self {
            twilio_auth_token: crate::server::provider::get(command, "twilio_auth_token")
                .unwrap_or_default(),
            public_base_url: crate::server::provider::get(command, "brain_receiver_public_url")
                .unwrap_or_default(),
            resend_signing_secret: crate::server::provider::get(
                command,
                "resend_webhook_signing_secret",
            )
            .unwrap_or_default(),
            resend_api_key: crate::server::provider::get(command, "resend_api_key")
                .unwrap_or_default(),
            workspace_id: command.workspace.id(),
            local_user_id,
            users,
        })
    }

    fn resolve_actor(
        &self,
        provider_authenticated: bool,
        identity: crate::actor::RequestIdentity<'_>,
    ) -> Result<crate::actor::ActorContext, crate::server::security::AuthenticatedActorError> {
        crate::server::security::resolve_authenticated_actor(
            provider_authenticated,
            &self.local_user_id,
            identity,
            &self.users,
        )
    }
}

#[derive(Default)]
struct RecentMessageIds {
    order: std::collections::VecDeque<(Channel, String)>,
    ids: std::collections::HashSet<(Channel, String)>,
}

impl RecentMessageIds {
    fn contains(&self, channel: Channel, provider_id: Option<&str>) -> bool {
        provider_id.is_some_and(|id| self.ids.contains(&(channel, id.to_owned())))
    }

    fn record(&mut self, channel: Channel, provider_id: Option<&str>) {
        let Some(provider_id) = provider_id else {
            return;
        };
        let key = (channel, provider_id.to_owned());
        if !self.ids.insert(key.clone()) {
            return;
        }
        self.order.push_back(key);
        while self.order.len() > RECENT_PROVIDER_IDS {
            if let Some(expired) = self.order.pop_front() {
                self.ids.remove(&expired);
            }
        }
    }
}

pub struct ReceiverServer {
    server: Arc<Server>,
    joins: Vec<JoinHandle<()>>,
}

impl ReceiverServer {
    /// Bind and start the TUI-owned listener.
    pub fn start(
        command: &crate::workspace::CommandContext,
        port: u16,
        tx: &SyncSender<InboundMessage>,
    ) -> Result<Self> {
        let server = Arc::new(
            Server::http(("127.0.0.1", port))
                .map_err(|error| anyhow::anyhow!("binding receiver server: {error}"))?,
        );
        let actual = server
            .server_addr()
            .to_ip()
            .context("resolving receiver server address")?
            .port();
        let security = SecurityConfig::load(command)?;
        let recent = Arc::new(Mutex::new(RecentMessageIds::default()));
        let mut joins = Vec::with_capacity(RECEIVER_WORKERS);
        for index in 0..RECEIVER_WORKERS {
            let worker_server = Arc::clone(&server);
            let worker_tx = SyncSender::clone(tx);
            let worker_security = security.clone();
            let worker_recent = Arc::clone(&recent);
            let join = thread::Builder::new()
                .name(format!("brain-receiver-{}", index + 1))
                .spawn(move || {
                    crate::logging::log(format!("receiver worker {} started", index + 1));
                    loop {
                        match worker_server.recv() {
                            Ok(mut request) => {
                                let response = respond(
                                    &mut request,
                                    &worker_tx,
                                    &worker_security,
                                    &worker_recent,
                                );
                                if let Err(error) = request.respond(response) {
                                    crate::logging::log(format!(
                                        "receiver response write failed: {error}"
                                    ));
                                }
                            }
                            Err(error) => {
                                crate::logging::log(format!(
                                    "receiver worker {} receive stopped: {error}",
                                    index + 1
                                ));
                                break;
                            }
                        }
                    }
                    crate::logging::log(format!("receiver worker {} stopped", index + 1));
                })
                .context("starting receiver server thread")?;
            joins.push(join);
        }
        crate::logging::log(format!(
            "receiver server bound port={actual} workers={RECEIVER_WORKERS}"
        ));
        Ok(Self { server, joins })
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.joins.iter().any(|worker| !worker.is_finished())
    }
}

impl Drop for ReceiverServer {
    fn drop(&mut self) {
        for _ in 0..self.joins.len() {
            self.server.unblock();
        }
        for join in self.joins.drain(..) {
            let _ = join.join();
        }
    }
}

fn respond(
    request: &mut tiny_http::Request,
    tx: &SyncSender<InboundMessage>,
    security: &SecurityConfig,
    recent: &Mutex<RecentMessageIds>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let route = crate::server::router::route(request.method().as_str(), request.url());
    crate::logging::log(format!(
        "receiver http request method={} url={} route={route:?}",
        request.method(),
        request.url()
    ));
    let result = match route {
        Route::Sms => read_request_body(request)
            .and_then(|body| sms::parse_sms(request, &body, security))
            .and_then(|message| enqueue(tx, message, recent)),
        Route::Email => read_request_body(request)
            .and_then(|body| email::parse_email(request, &body, security))
            .and_then(|message| enqueue(tx, message, recent)),
        _ => Err((404, "not found".to_owned())),
    };
    match result {
        Ok(()) if route == Route::Sms => {
            crate::logging::log("receiver http accepted route=Sms status=200");
            Response::from_string(
                "<Response><Message>Received. I’ll get back to you shortly.</Message></Response>",
            )
            .with_status_code(200)
            .with_header(xml_header())
        }
        Ok(()) => {
            crate::logging::log(format!("receiver http accepted route={route:?} status=200"));
            Response::from_string("{\"ok\":true,\"queued\":true}")
                .with_status_code(200)
                .with_header(json_header())
        }
        Err((status, error)) => {
            crate::logging::log(format!(
                "receiver http rejected route={route:?} status={status} error={error}"
            ));
            Response::from_string(format!(
                "{{\"ok\":false,\"error\":{}}}",
                serde_json::to_string(&error).unwrap_or_else(|_| "\"request rejected\"".to_owned())
            ))
            .with_status_code(status)
            .with_header(json_header())
        }
    }
}

fn read_request_body(request: &mut tiny_http::Request) -> Result<Vec<u8>, (u16, String)> {
    let length = request.body_length();
    read_body_from_reader(request.as_reader(), length)
}

fn read_body_from_reader(
    reader: &mut dyn Read,
    length: Option<usize>,
) -> Result<Vec<u8>, (u16, String)> {
    if length.is_some_and(|length| length > MAX_REQUEST_BODY_BYTES) {
        return Err((413, "webhook body is too large".to_owned()));
    }
    let read_limit = length.unwrap_or(MAX_REQUEST_BODY_BYTES.saturating_add(1));
    let mut body = Vec::with_capacity(read_limit.min(MAX_REQUEST_BODY_BYTES));
    reader
        .take(u64::try_from(read_limit).unwrap_or(u64::MAX))
        .read_to_end(&mut body)
        .map_err(|_| (400, "could not read webhook body".to_owned()))?;
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return Err((413, "webhook body is too large".to_owned()));
    }
    Ok(body)
}

fn enqueue(
    tx: &SyncSender<InboundMessage>,
    message: InboundMessage,
    recent: &Mutex<RecentMessageIds>,
) -> Result<(), (u16, String)> {
    let mut recent = recent
        .lock()
        .map_err(|_| (503, "brain receiver state is unavailable".to_owned()))?;
    if recent.contains(message.channel, message.provider_id.as_deref()) {
        crate::logging::log(format!(
            "receiver duplicate ignored channel={:?} provider_id={}",
            message.channel,
            message.provider_id.as_deref().unwrap_or_default()
        ));
        return Ok(());
    }
    let channel = message.channel;
    let provider_id = message.provider_id.clone();
    match tx.try_send(message) {
        Ok(()) => {
            recent.record(channel, provider_id.as_deref());
            drop(recent);
            Ok(())
        }
        Err(TrySendError::Full(_)) => {
            Err((503, "brain receiver queue is full; retry later".to_owned()))
        }
        Err(TrySendError::Disconnected(_)) => {
            Err((503, "brain is not accepting messages".to_owned()))
        }
    }
}

fn json_header() -> Header {
    Header::from_bytes(&b"Content-Type"[..], b"application/json")
        .unwrap_or_else(|()| unreachable!("static header is valid"))
}

fn xml_header() -> Header {
    Header::from_bytes(&b"Content-Type"[..], b"application/xml")
        .unwrap_or_else(|()| unreachable!("static header is valid"))
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{ReceiverServer, RecentMessageIds, enqueue, read_body_from_reader};
    use crate::server::receiver::{Channel, INBOUND_QUEUE_CAPACITY, InboundMessage};

    struct CommandFixture {
        _temp: tempfile::TempDir,
        command: crate::workspace::CommandContext,
    }

    fn command() -> CommandFixture {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("brain");
        std::fs::create_dir_all(&root).unwrap();
        let workspace = std::sync::Arc::new(
            crate::workspace::WorkspaceContext::new(
                temp.path(),
                crate::workspace::WorkspaceId::new(),
                crate::workspace::WorkspaceName::parse("brain").expect("valid name"),
                &root,
                "tester",
                temp.path(),
            )
            .expect("context"),
        );
        crate::users::UsersStore::save(
            &workspace,
            &crate::users::Users {
                schema_version: crate::users::USERS_SCHEMA_VERSION,
                users: vec![crate::users::User {
                    id: crate::users::UserId::parse("tester").unwrap(),
                    name: "Tester".to_owned(),
                    phones: Vec::new(),
                    emails: Vec::new(),
                    response_email: None,
                }],
            },
        )
        .unwrap();
        CommandFixture {
            _temp: temp,
            command: crate::workspace::CommandContext {
                workspace,
                registry_store: crate::workspace::RegistryStore::from_path(
                    std::path::PathBuf::from("/missing/env.json"),
                ),
            },
        }
    }

    fn message(provider_id: &str) -> InboundMessage {
        InboundMessage {
            workspace_id: crate::workspace::WorkspaceId::new(),
            actor: crate::actor::resolve_actor(
                &crate::users::UserId::parse("tester").unwrap(),
                crate::actor::RequestIdentity::Local,
                &crate::users::Users {
                    schema_version: crate::users::USERS_SCHEMA_VERSION,
                    users: vec![crate::users::User {
                        id: crate::users::UserId::parse("tester").unwrap(),
                        name: "Tester".to_owned(),
                        phones: Vec::new(),
                        emails: Vec::new(),
                        response_email: None,
                    }],
                },
            )
            .unwrap(),
            channel: Channel::Sms,
            body: "hello".to_owned(),
            sender: "+15551234567".to_owned(),
            participants: vec!["+15551234567".to_owned()],
            provider_id: Some(provider_id.to_owned()),
            attachments: Vec::new(),
        }
    }

    #[test]
    fn provider_delivery_ids_are_deduplicated_per_channel() {
        let mut recent = RecentMessageIds::default();

        assert!(!recent.contains(Channel::Email, Some("delivery-1")));
        recent.record(Channel::Email, Some("delivery-1"));
        assert!(recent.contains(Channel::Email, Some("delivery-1")));
        assert!(!recent.contains(Channel::Sms, Some("delivery-1")));
        assert!(!recent.contains(Channel::Email, None));
    }

    #[test]
    fn duplicate_delivery_succeeds_without_growing_the_queue() {
        let (tx, rx) = mpsc::sync_channel(1);
        let recent = std::sync::Mutex::new(RecentMessageIds::default());

        assert_eq!(enqueue(&tx, message("SM1"), &recent), Ok(()));
        assert_eq!(enqueue(&tx, message("SM1"), &recent), Ok(()));

        assert_eq!(rx.try_iter().count(), 1);
    }

    #[test]
    fn full_queue_returns_retryable_service_unavailable() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let recent = std::sync::Mutex::new(RecentMessageIds::default());
        enqueue(&tx, message("SM1"), &recent).unwrap();

        let error = enqueue(&tx, message("SM2"), &recent).unwrap_err();

        assert_eq!(error.0, 503);
        assert!(error.1.contains("retry later"));
    }

    #[test]
    fn request_body_reader_stops_at_content_length() {
        let mut reader = Cursor::new(b"body followed by keep-alive bytes".to_vec());
        let body = read_body_from_reader(&mut reader, Some(4)).unwrap();
        assert_eq!(body, b"body");
    }

    #[test]
    fn oversized_webhook_body_is_rejected() {
        let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let (message_tx, _message_rx) = mpsc::sync_channel(INBOUND_QUEUE_CAPACITY);
        let command = command();
        let receiver = ReceiverServer::start(&command.command, port, &message_tx).unwrap();
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let body = vec![b'x'; 1_048_577];
        write!(
            client,
            "POST /sms HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        client.write_all(&body).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        drop(receiver);

        assert!(response.starts_with("HTTP/1.1 413"), "{response}");
    }

    #[test]
    fn one_slow_webhook_does_not_block_other_requests() {
        let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let (message_tx, _message_rx) = mpsc::sync_channel(INBOUND_QUEUE_CAPACITY);
        let command = command();
        let receiver = ReceiverServer::start(&command.command, port, &message_tx).unwrap();
        let mut slow = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        slow.write_all(
            b"POST /sms HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let mut fast = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        fast.set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        fast.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut response = [0_u8; 128];
        let fast_response = fast.read(&mut response);
        slow.write_all(b"test").unwrap();
        drop(receiver);

        assert!(
            fast_response.is_ok_and(|read| {
                String::from_utf8_lossy(&response[..read]).starts_with("HTTP/1.1 404")
            }),
            "a slow request blocked the receiver worker"
        );
    }

    #[test]
    fn dropping_receiver_unblocks_and_joins_the_workers() {
        let (message_tx, _message_rx) = mpsc::sync_channel(INBOUND_QUEUE_CAPACITY);
        let command = command();
        let receiver = ReceiverServer::start(&command.command, 0, &message_tx).unwrap();
        let (done_tx, done_rx) = mpsc::channel();

        std::thread::spawn(move || {
            drop(receiver);
            let _ = done_tx.send(());
        });

        assert_eq!(done_rx.recv_timeout(Duration::from_secs(1)), Ok(()));
    }
}
