use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac as _};
use sha1::Sha1;
use sha2::Sha256;

pub struct ProviderPost {
    path: String,
    headers: String,
    body: String,
}

pub fn signed_sms(
    ingress: brain::server::IngressId,
    token: &str,
    provider_id: &str,
    prompt: &str,
    sender: &str,
) -> ProviderPost {
    let fields = BTreeMap::from([
        ("Body".to_owned(), prompt.to_owned()),
        ("From".to_owned(), sender.to_owned()),
        ("MessageSid".to_owned(), provider_id.to_owned()),
    ]);
    let path = format!("/w/{ingress}/sms");
    let signature_url = format!("https://receiver.example.test{path}");
    let signature = twilio_signature(token, &signature_url, &fields);
    let body = format!(
        "Body={}&From={}&MessageSid={provider_id}",
        prompt.replace(' ', "+"),
        sender.replace('+', "%2B")
    );
    ProviderPost {
        path,
        headers: format!("X-Twilio-Signature: {signature}\r\n"),
        body,
    }
}

pub fn signed_email_event(
    ingress: brain::server::IngressId,
    secret: &[u8],
    webhook_id: &str,
    event_type: &str,
) -> ProviderPost {
    let body = format!(r#"{{"type":"{event_type}","data":{{"from":"member@example.test"}}}}"#);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(webhook_id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    let signature = format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()));
    ProviderPost {
        path: format!("/w/{ingress}/email"),
        headers: format!(
            "svix-id: {webhook_id}\r\nsvix-timestamp: {timestamp}\r\nsvix-signature: {signature}\r\n"
        ),
        body,
    }
}

pub fn post(port: u16, request: &ProviderPost) -> String {
    let wire = format!(
        "POST {} HTTP/1.1\r\nHost: localhost\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        request.path,
        request.headers,
        request.body.len(),
        request.body
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(wire.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn twilio_signature(token: &str, url: &str, fields: &BTreeMap<String, String>) -> String {
    let mut payload = url.to_owned();
    for (key, value) in fields {
        payload.push_str(key);
        payload.push_str(value);
    }
    let mut mac = Hmac::<Sha1>::new_from_slice(token.as_bytes()).unwrap();
    mac.update(payload.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}
