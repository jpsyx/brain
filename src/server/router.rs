//! Pure routing for the brain server: an HTTP method + URL path in, a
//! [`Route`] out. No IO, no state, just the mapping the accept loop
//! dispatches on, so it can be unit-tested exhaustively.

use crate::server::IngressId;

/// A resolved brain-server route. Everything the server does NOT recognize
/// (unknown paths, the bare root `/`, wrong methods) collapses to
/// [`Route::NotFound`]; the brain server has no root view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `GET /w/<ingress>/habits`: the selected workspace's habits page.
    HabitsPage { ingress: IngressId },
    /// `POST /w/<ingress>/habits/done`: mark a selected workspace habit done.
    HabitsDone { ingress: IngressId },
    /// `POST /w/<ingress>/sms`: receive an authenticated Twilio webhook.
    Sms { ingress: IngressId },
    /// `POST /w/<ingress>/email`: receive an authenticated Resend webhook.
    Email { ingress: IngressId },
    /// `POST /w/<ingress>/triage/done`: report a selected workspace's
    /// ephemeral daily-triage session complete.
    TriageDone { ingress: IngressId },
    /// Anything else.
    NotFound,
}

/// Map an HTTP method + URL path to a brain-server route. Pure.
///
/// Any query string is stripped before matching. Every accepted route has an
/// exact `/w/<opaque ingress>/...` component shape.
#[must_use]
pub fn route(method: &str, path: &str) -> Route {
    let path = path.split('?').next().unwrap_or(path);
    let mut components = path.split('/');
    match (
        components.next(),
        components.next(),
        components.next(),
        components.next(),
        components.next(),
        components.next(),
    ) {
        (Some(""), Some("w"), Some(raw_ingress), Some(first), second, None) => {
            let Ok(ingress) = IngressId::parse(raw_ingress) else {
                return Route::NotFound;
            };
            match (method, first, second) {
                ("GET", "habits", None) => Route::HabitsPage { ingress },
                ("POST", "habits", Some("done")) => Route::HabitsDone { ingress },
                ("POST", "triage", Some("done")) => Route::TriageDone { ingress },
                ("POST", "sms", None) => Route::Sms { ingress },
                ("POST", "email", None) => Route::Email { ingress },
                _ => Route::NotFound,
            }
        }
        _ => Route::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INGRESS: &str = "4ea7480a-bd86-47ec-9372-9f765ac2113a";

    fn ingress() -> crate::server::IngressId {
        crate::server::IngressId::parse(INGRESS).expect("valid ingress fixture")
    }

    #[test]
    fn ingress_routes_every_supported_endpoint() {
        let cases = [
            (
                "GET",
                format!("/w/{INGRESS}/habits"),
                Route::HabitsPage { ingress: ingress() },
            ),
            (
                "POST",
                format!("/w/{INGRESS}/habits/done"),
                Route::HabitsDone { ingress: ingress() },
            ),
            (
                "POST",
                format!("/w/{INGRESS}/triage/done"),
                Route::TriageDone { ingress: ingress() },
            ),
            (
                "POST",
                format!("/w/{INGRESS}/sms"),
                Route::Sms { ingress: ingress() },
            ),
            (
                "POST",
                format!("/w/{INGRESS}/email"),
                Route::Email { ingress: ingress() },
            ),
        ];

        for (method, path, expected) in cases {
            assert_eq!(route(method, &path), expected, "{method} {path}");
        }
    }

    #[test]
    fn query_is_stripped_after_the_ingress_route_is_parsed() {
        assert_eq!(
            route("GET", &format!("/w/{INGRESS}/habits?view=today")),
            Route::HabitsPage { ingress: ingress() }
        );
    }

    #[test]
    fn global_and_malformed_routes_are_not_found() {
        for (method, path) in [
            ("GET", "/habits"),
            ("POST", "/habits/done"),
            ("POST", "/triage/done"),
            ("POST", "/sms"),
            ("POST", "/email"),
            ("GET", "/w/habits"),
            ("GET", "/w/not-a-uuid/habits"),
            ("GET", "/w//habits"),
            ("GET", "/w/4ea7480a-bd86-47ec-9372-9f765ac2113a"),
            (
                "GET",
                "/w/4ea7480a-bd86-47ec-9372-9f765ac2113a/habits/extra",
            ),
            ("POST", "/w/4ea7480a-bd86-47ec-9372-9f765ac2113a/habits"),
            ("GET", "/w/4ea7480a-bd86-47ec-9372-9f765ac2113a/habits/done"),
        ] {
            assert_eq!(route(method, path), Route::NotFound, "{method} {path}");
        }
    }
}
