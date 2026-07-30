use std::collections::BTreeSet;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::config::Config;

#[must_use]
pub(crate) fn receiver_phone_warning(config: &Config, twilio_from: Option<&str>) -> Option<String> {
    let invalid = config
        .allowed_sms()
        .into_iter()
        .chain(
            twilio_from
                .map(str::trim)
                .filter(|number| !number.is_empty())
                .map(str::to_owned),
        )
        .filter(|number| !crate::server::security::is_e164_phone_number(number))
        .collect::<BTreeSet<_>>();
    if invalid.is_empty() {
        return None;
    }
    let numbers = invalid.into_iter().collect::<Vec<_>>().join(", ");
    Some(format!(
        "⚠ SMS phone number is malformed and needs a country code: {numbers}. Use E.164, for example +16072809118"
    ))
}

#[must_use]
pub(crate) fn persistent_warning_line(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(
            message.to_owned(),
            Style::default()
                .fg(Color::Rgb(255, 199, 119))
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

#[must_use]
pub(crate) fn sync_status_line(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(
            message.to_owned(),
            Style::default()
                .fg(Color::Rgb(130, 218, 255))
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

#[must_use]
pub(crate) fn status_override_line(
    flash: Option<&super::FlashKind>,
    sync_status: Option<&str>,
    persistent_warning: Option<&str>,
) -> Option<Line<'static>> {
    flash
        .map(super::flash_line)
        .or_else(|| sync_status.map(sync_status_line))
        .or_else(|| persistent_warning.map(persistent_warning_line))
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::{persistent_warning_line, receiver_phone_warning, status_override_line};
    use crate::config::Config;
    use crate::tui::FlashKind;

    #[test]
    fn malformed_sms_number_produces_a_country_code_warning() {
        let config: Config =
            serde_json::from_str(r#"{"allowed_sms_senders":"6072809118"}"#).unwrap();

        assert_eq!(
            receiver_phone_warning(&config, None).as_deref(),
            Some(
                "⚠ SMS phone number is malformed and needs a country code: 6072809118. Use E.164, for example +16072809118"
            )
        );
    }

    #[test]
    fn valid_e164_numbers_do_not_produce_a_warning() {
        let config: Config =
            serde_json::from_str(r#"{"allowed_sms_senders":"+16072809118"}"#).unwrap();

        assert_eq!(receiver_phone_warning(&config, Some("+15551234567")), None);
    }

    #[test]
    fn persistent_phone_warning_is_rendered_in_yellow() {
        let line = persistent_warning_line("phone warning");

        assert_eq!(line.to_string(), " phone warning");
        assert_eq!(line.spans[1].style.fg, Some(Color::Rgb(255, 199, 119)));
    }

    #[test]
    fn persistent_warning_returns_after_a_transient_flash_clears() {
        let flash = FlashKind::Info("saved".to_owned());

        assert_eq!(
            status_override_line(Some(&flash), None, Some("phone warning"))
                .unwrap()
                .to_string(),
            " saved"
        );
        assert_eq!(
            status_override_line(None, None, Some("phone warning"))
                .unwrap()
                .to_string(),
            " phone warning"
        );
    }

    #[test]
    fn active_sync_status_takes_priority_over_persistent_warnings() {
        assert_eq!(
            status_override_line(None, Some("syncing brain (pull)…"), Some("phone warning"))
                .unwrap()
                .to_string(),
            " syncing brain (pull)…"
        );
    }
}
