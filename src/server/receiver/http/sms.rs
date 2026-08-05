use std::collections::{BTreeSet, HashMap};

use tiny_http::{Method, Request};

use super::SecurityConfig;
use crate::server::receiver::{Attachment, Channel, InboundMessage};

pub(super) fn parse_sms(
    request: &Request,
    body: &[u8],
    security: &SecurityConfig,
) -> Result<InboundMessage, (u16, String)> {
    if request.method() != &Method::Post {
        return Err((405, "method not allowed".to_owned()));
    }
    let fields = parse_form(body)?;
    if security.twilio_auth_token.is_empty() || security.public_base_url.is_empty() {
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
    let authenticated = crate::server::security::verify_twilio(
        &security.twilio_auth_token,
        &twilio_signature_url(&security.public_base_url, security.ingress_id),
        &sorted,
        signature,
    );
    let text = fields.get("Body").cloned().unwrap_or_default();
    let sender = fields.get("From").cloned().unwrap_or_default();
    let actor = security
        .resolve_actor(
            authenticated,
            crate::actor::RequestIdentity::Sms { from: &sender },
        )
        .map_err(|error| match error {
            crate::server::security::AuthenticatedActorError::ProviderAuthenticationFailed => {
                (403, "invalid Twilio signature".to_owned())
            }
            crate::server::security::AuthenticatedActorError::UnknownOrDisallowedSender => {
                (403, "SMS sender is not allowed".to_owned())
            }
        })?;
    let sender = crate::users::normalize_phone(&sender)
        .map_err(|_| (403, "SMS sender is not allowed".to_owned()))?;
    if text.trim().is_empty() && !fields.keys().any(|key| key.starts_with("MediaUrl")) {
        return Err((400, "SMS body and media are both empty".to_owned()));
    }
    Ok(InboundMessage {
        workspace_id: security.workspace_id,
        actor,
        channel: Channel::Sms,
        body: text,
        sender: sender.clone(),
        participants: vec![sender],
        provider_id: fields.get("MessageSid").cloned(),
        attachments: sms_attachments(&fields),
    })
}

fn twilio_signature_url(public_base_url: &str, ingress: crate::server::IngressId) -> String {
    format!("{}/w/{ingress}/sms", public_base_url.trim_end_matches('/'))
}

fn sms_attachments(fields: &HashMap<String, String>) -> Vec<Attachment> {
    let indices = fields
        .keys()
        .filter_map(|key| key.strip_prefix("MediaUrl"))
        .filter_map(|index| index.parse::<usize>().ok())
        .collect::<BTreeSet<_>>();
    indices
        .into_iter()
        .filter_map(|index| {
            let url = fields.get(&format!("MediaUrl{index}"))?.clone();
            Some(Attachment {
                url,
                content_type: fields.get(&format!("MediaContentType{index}")).cloned(),
                filename: None,
            })
        })
        .collect()
}

fn parse_form(body: &[u8]) -> Result<HashMap<String, String>, (u16, String)> {
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
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2]))
        {
            out.push((high << 4) | low);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_form, sms_attachments, twilio_signature_url};
    use std::collections::HashMap;

    const FAMILY_INGRESS: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

    #[test]
    fn twilio_signature_url_includes_the_resolved_ingress() {
        let ingress = crate::server::IngressId::parse(FAMILY_INGRESS).unwrap();

        assert_eq!(
            twilio_signature_url("https://receiver.example/", ingress),
            format!("https://receiver.example/w/{FAMILY_INGRESS}/sms")
        );
    }

    #[test]
    fn mms_media_keeps_twilio_index_order_and_content_types() {
        let fields = HashMap::from([
            ("NumMedia".to_owned(), "2".to_owned()),
            (
                "MediaUrl1".to_owned(),
                "https://api.twilio.test/media/second".to_owned(),
            ),
            ("MediaContentType1".to_owned(), "image/png".to_owned()),
            (
                "MediaUrl0".to_owned(),
                "https://api.twilio.test/media/first".to_owned(),
            ),
            ("MediaContentType0".to_owned(), "image/jpeg".to_owned()),
        ]);

        let attachments = sms_attachments(&fields);

        assert_eq!(attachments.len(), 2);
        assert!(attachments[0].url.ends_with("/first"));
        assert_eq!(attachments[0].content_type.as_deref(), Some("image/jpeg"));
        assert!(attachments[1].url.ends_with("/second"));
        assert_eq!(attachments[1].content_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn malformed_percent_encoding_cannot_panic_the_receiver() {
        assert_eq!(decode_form("%aé"), "%aé");
    }
}
