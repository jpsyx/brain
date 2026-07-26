//! Pure routing for the brain server: an HTTP method + URL path in, a
//! [`Route`] out. No IO, no state, just the mapping the accept loop
//! dispatches on, so it can be unit-tested exhaustively.

/// A resolved brain-server route. Everything the server does NOT recognize
/// (unknown paths, the bare root `/`, wrong methods) collapses to
/// [`Route::NotFound`]; the brain server has no root view.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// `GET /habits`: the habits page.
    HabitsPage,
    /// `POST /habits/done`: mark a habit done.
    HabitsDone,
    /// `POST /webhooks/capture`: store an inbound webhook payload for triage.
    WebhookCapture,
    /// Anything else.
    NotFound,
}

/// Map an HTTP method + URL path to a brain-server route. Pure.
///
/// Any query string is stripped before matching, so `/habits?x=1` routes the
/// same as `/habits`.
#[must_use]
pub fn route(method: &str, path: &str) -> Route {
    let path = path.split('?').next().unwrap_or(path);
    match (method, path) {
        ("GET", "/habits") => Route::HabitsPage,
        ("POST", "/habits/done") => Route::HabitsDone,
        ("POST", "/webhooks/capture") => Route::WebhookCapture,
        _ => Route::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_habits_is_the_habits_page() {
        assert_eq!(route("GET", "/habits"), Route::HabitsPage);
    }

    #[test]
    fn post_habits_done_is_habits_done() {
        assert_eq!(route("POST", "/habits/done"), Route::HabitsDone);
    }

    #[test]
    fn post_webhooks_capture_is_the_capture_endpoint() {
        assert_eq!(route("POST", "/webhooks/capture"), Route::WebhookCapture);
    }

    #[test]
    fn root_is_not_found() {
        assert_eq!(route("GET", "/"), Route::NotFound);
    }

    #[test]
    fn query_string_is_stripped_before_matching() {
        assert_eq!(route("GET", "/habits?x=1"), Route::HabitsPage);
    }

    #[test]
    fn post_habits_without_done_is_not_found() {
        assert_eq!(route("POST", "/habits"), Route::NotFound);
    }

    #[test]
    fn wrong_method_on_habits_is_not_found() {
        assert_eq!(route("POST", "/habits"), Route::NotFound);
        assert_eq!(route("GET", "/habits/done"), Route::NotFound);
    }

    #[test]
    fn unknown_path_is_not_found() {
        assert_eq!(route("GET", "/nope"), Route::NotFound);
    }
}
