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
    /// `POST /sms`: receive a Twilio webhook. The workspace is selected by the
    /// number the message arrived at, never by the URL.
    Sms,
    /// `POST /email`: receive a Resend webhook. The workspace is selected by the
    /// address the message arrived at, never by the URL.
    Email,
    /// `POST /local/<lease>/w/<ingress>/session/done`: report one of a
    /// workspace's ephemeral skill sessions complete.
    SkillSessionDone {
        ingress: IngressId,
        capability: crate::server::lifecycle::LeaseId,
    },
    /// Anything else.
    NotFound,
}

/// Map an HTTP method + URL path to a brain-server route. Pure.
///
/// Any query string is stripped before matching. Provider routes are the two
/// machine-wide `/sms` and `/email` paths, carrying no workspace identity at
/// all; local actions require an ingress plus a lease capability.
#[must_use]
pub fn route(method: &str, path: &str) -> Route {
    let path = path.split('?').next().unwrap_or(path);
    if let Some(route) = local_route(method, path) {
        return route;
    }
    match (method, path) {
        ("POST", "/sms") => Route::Sms,
        ("POST", "/email") => Route::Email,
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
        ("POST", "session", ["done"]) => Some(Route::SkillSessionDone {
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
    fn one_machine_wide_path_serves_each_provider_channel() {
        // Every workspace's provider portal is pointed at the same URL; the
        // destination number or address inside the payload selects the
        // workspace, so the path carries no identity to leak or to get wrong.
        assert_eq!(route("POST", "/sms"), Route::Sms);
        assert_eq!(route("POST", "/email"), Route::Email);
    }

    #[test]
    fn a_provider_channel_answers_only_a_post() {
        for method in ["GET", "PUT", "DELETE", "HEAD"] {
            assert_eq!(route(method, "/sms"), Route::NotFound, "{method} /sms");
            assert_eq!(route(method, "/email"), Route::NotFound, "{method} /email");
        }
    }

    #[test]
    fn the_retired_ingress_scoped_provider_paths_are_gone() {
        // A stale portal entry must fail loudly rather than reach a workspace
        // the new address routing never agreed to.
        for suffix in ["sms", "email"] {
            let path = format!("/w/{INGRESS}/{suffix}");
            assert_eq!(route("POST", &path), Route::NotFound, "POST {path}");
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
                "session/done",
                Route::SkillSessionDone {
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
            ("POST", format!("/w/{INGRESS}/session/done")),
        ] {
            assert_eq!(route(method, &path), Route::NotFound, "{method} {path}");
        }
    }

    #[test]
    fn query_is_stripped_before_the_route_is_parsed() {
        assert_eq!(
            route("GET", &format!("/w/{INGRESS}/habits?view=today")),
            Route::NotFound
        );
        // Twilio must never append one, but stripping it here keeps the exact
        // rejection where the signature is checked against the literal URL.
        assert_eq!(route("POST", "/sms?unexpected=1"), Route::Sms);
    }

    #[test]
    fn global_and_malformed_routes_are_not_found() {
        for (method, path) in [
            ("GET", "/habits"),
            ("POST", "/habits/done"),
            ("POST", "/session/done"),
            ("POST", "/sms/"),
            ("POST", "/email/extra"),
            ("POST", "/"),
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
