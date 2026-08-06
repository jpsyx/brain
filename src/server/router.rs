//! Pure routing for the brain server: an HTTP method + URL path in, a
//! [`Route`] out. No IO, no state, just the mapping the accept loop
//! dispatches on, so it can be unit-tested exhaustively.

use crate::server::IngressId;

/// A resolved brain-server route. Everything the server does NOT recognize
/// (unknown paths, the bare root `/`, wrong methods) collapses to
/// [`Route::NotFound`]; the brain server has no root view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `GET /local/<lease>/w/<ingress>/habits`: the selected workspace's habits page.
    HabitsPage {
        ingress: IngressId,
        capability: crate::server::lifecycle::LeaseId,
    },
    /// `POST /local/<lease>/w/<ingress>/habits/done`: mark a selected habit done.
    HabitsDone {
        ingress: IngressId,
        capability: crate::server::lifecycle::LeaseId,
    },
    /// `POST /w/<ingress>/sms`: receive an authenticated Twilio webhook.
    Sms { ingress: IngressId },
    /// `POST /w/<ingress>/email`: receive an authenticated Resend webhook.
    Email { ingress: IngressId },
    /// `POST /local/<lease>/w/<ingress>/triage/done`: report a workspace's
    /// ephemeral daily-triage session complete.
    TriageDone {
        ingress: IngressId,
        capability: crate::server::lifecycle::LeaseId,
    },
    /// Anything else.
    NotFound,
}

/// Map an HTTP method + URL path to a brain-server route. Pure.
///
/// Any query string is stripped before matching. Provider routes have an exact
/// `/w/<opaque ingress>/...` shape; local actions also require a lease capability.
#[must_use]
pub fn route(method: &str, path: &str) -> Route {
    let path = path.split('?').next().unwrap_or(path);
    if let Some(route) = local_route(method, path) {
        return route;
    }
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
                ("POST", "sms", None) => Route::Sms { ingress },
                ("POST", "email", None) => Route::Email { ingress },
                _ => Route::NotFound,
            }
        }
        _ => Route::NotFound,
    }
}

fn local_route(method: &str, path: &str) -> Option<Route> {
    let components = path.split('/').collect::<Vec<_>>();
    let [
        "",
        "local",
        raw_capability,
        "w",
        raw_ingress,
        first,
        rest @ ..,
    ] = components.as_slice()
    else {
        return None;
    };
    let capability = crate::server::lifecycle::LeaseId::parse(raw_capability).ok()?;
    let ingress = IngressId::parse(raw_ingress).ok()?;
    match (method, *first, rest) {
        ("GET", "habits", []) => Some(Route::HabitsPage {
            ingress,
            capability,
        }),
        ("POST", "habits", ["done"]) => Some(Route::HabitsDone {
            ingress,
            capability,
        }),
        ("POST", "triage", ["done"]) => Some(Route::TriageDone {
            ingress,
            capability,
        }),
        _ => None,
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
    fn local_actions_require_the_exact_live_lease_capability() {
        let capability =
            crate::server::lifecycle::LeaseId::parse("57b162df-983a-45c3-ac7e-bad94eb27a99")
                .unwrap();
        for (method, suffix, expected) in [
            (
                "GET",
                "habits",
                Route::HabitsPage {
                    ingress: ingress(),
                    capability,
                },
            ),
            (
                "POST",
                "habits/done",
                Route::HabitsDone {
                    ingress: ingress(),
                    capability,
                },
            ),
            (
                "POST",
                "triage/done",
                Route::TriageDone {
                    ingress: ingress(),
                    capability,
                },
            ),
        ] {
            let path = format!("/local/{capability}/w/{INGRESS}/{suffix}");
            assert_eq!(route(method, &path), expected, "{method} {path}");
        }

        for (method, path) in [
            ("GET", format!("/w/{INGRESS}/habits")),
            ("POST", format!("/w/{INGRESS}/habits/done")),
            ("POST", format!("/w/{INGRESS}/triage/done")),
        ] {
            assert_eq!(route(method, &path), Route::NotFound, "{method} {path}");
        }
    }

    #[test]
    fn query_is_stripped_after_the_ingress_route_is_parsed() {
        assert_eq!(
            route("GET", &format!("/w/{INGRESS}/habits?view=today")),
            Route::NotFound
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
