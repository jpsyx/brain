use std::collections::{BTreeSet, HashMap};

use super::{AuthenticatedInbound, ProviderConfig, ProviderError};
use crate::server::receiver::{AttachmentRef, Channel};

pub(super) fn authenticate(
    request: &crate::server::http::Request,
    body: &[u8],
    config: &ProviderConfig,
) -> Result<AuthenticatedInbound, ProviderError> {
    if config.twilio_auth_token.is_empty() || config.public_base_url.is_empty() {
        return Err(ProviderError::NotConfigured(
            "Twilio security is not configured",
        ));
    }
    let fields = parse_form(body)?;
    let sorted = fields
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let signature = request.header("x-twilio-signature").unwrap_or_default();
    if !crate::server::security::verify_twilio(
        &config.twilio_auth_token,
        &twilio_signature_url(&config.public_base_url, config.ingress_id),
        &sorted,
        signature,
    ) {
        return Err(ProviderError::InvalidSignature("invalid Twilio signature"));
    }
    let prompt = fields.get("Body").cloned().unwrap_or_default();
    let sender = fields.get("From").cloned().unwrap_or_default();
    let sender = crate::users::normalize_phone(&sender)
        .map_err(|_| ProviderError::SenderNotAllowed("SMS sender is not allowed"))?;
    let attachments = sms_attachments(&fields);
    if prompt.trim().is_empty() && attachments.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "SMS body and media are both empty",
        ));
    }
    Ok(AuthenticatedInbound {
        channel: Channel::Sms,
        sender: sender.clone(),
        prompt,
        participants: vec![sender],
        attachments,
        receiving_address: String::new(),
        provider_id: fields.get("MessageSid").cloned(),
    })
}

fn twilio_signature_url(public_base_url: &str, ingress: crate::server::IngressId) -> String {
    format!("{}/w/{ingress}/sms", public_base_url.trim_end_matches('/'))
}

fn sms_attachments(fields: &HashMap<String, String>) -> Vec<AttachmentRef> {
    fields
        .keys()
        .filter_map(|key| key.strip_prefix("MediaUrl"))
        .filter_map(|index| index.parse::<usize>().ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|index| {
            Some(AttachmentRef {
                url: fields.get(&format!("MediaUrl{index}"))?.clone(),
                content_type: fields.get(&format!("MediaContentType{index}")).cloned(),
                filename: None,
            })
        })
        .collect()
}

fn parse_form(body: &[u8]) -> Result<HashMap<String, String>, ProviderError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| ProviderError::InvalidRequest("SMS body is not UTF-8"))?;
    Ok(text
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (decode_form(key), decode_form(value)))
        .collect())
}

fn decode_form(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
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
    use std::collections::HashMap;

    use super::{decode_form, sms_attachments, twilio_signature_url};

    #[test]
    fn signature_url_contains_the_selected_ingress() {
        let ingress =
            crate::server::IngressId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
        assert_eq!(
            twilio_signature_url("https://receiver.example/", ingress),
            format!("https://receiver.example/w/{ingress}/sms")
        );
    }

    #[test]
    fn malformed_percent_encoding_is_preserved_without_panicking() {
        assert_eq!(decode_form("%aé"), "%aé");
    }

    #[test]
    fn mms_media_keeps_provider_index_order_and_content_types() {
        let fields = HashMap::from([
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
}
