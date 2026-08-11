//! The prompt a skill session is launched with: the workspace's own prompt plus
//! the completion protocol brain needs to close the tab.
//!
//! brain cannot assume the thing it launches knows anything about brain. A
//! configured session may point at any skill or any bare instruction, written by
//! someone who never heard of a skill-session tab. So the *protocol* travels
//! with the prompt: brain appends a short, explicit instruction to POST the
//! one-time token from [`TOKEN_ENV`] to the local route in [`DONE_URL_ENV`] as
//! the run's last action. That is the only reason the tab can close on
//! completion rather than on "the agent stopped talking", which is not the same
//! thing when a pass pauses to ask a question.
//!
//! The `require` list is part of the protocol, not knowledge about any
//! particular run: whatever output paths *this* run was told it must produce go
//! in it, and brain holds the tab open until each one exists. Nothing here knows
//! what those files are, and an empty list (the default) closes as soon as the
//! signal lands.

/// Env var carrying the ingress-scoped completion route for a skill session.
pub const DONE_URL_ENV: &str = "BRAIN_SESSION_DONE_URL";

/// Env var carrying a skill session's one-time completion token.
pub const TOKEN_ENV: &str = "BRAIN_SESSION_TOKEN";

/// The full prompt for a skill session: the configured prompt, then the
/// completion protocol. Pure.
#[must_use]
pub fn launch_prompt(prompt: &str) -> String {
    format!("{}\n\n{}", prompt.trim(), completion_protocol())
}

/// The appended completion-protocol instruction, verbatim.
#[must_use]
pub fn completion_protocol() -> String {
    format!(
        "---\n\
         You are running as a brain *skill session*: a dedicated agent session for this one \
         request, in its own brain-panel tab. brain closes that tab for you when the run is \
         genuinely finished, and it learns that only from you.\n\n\
         As your very last action — after everything above is complete, every question you \
         needed to ask has been answered, and anything you must show the user has been shown — \
         run:\n\
         ```sh\n\
         [ -n \"${DONE_URL_ENV}\" ] && curl -fsS -X POST \"${DONE_URL_ENV}\" \\\n\
         \x20 -H 'Content-Type: application/json' \\\n\
         \x20 -d \"{{\\\"token\\\": \\\"${TOKEN_ENV}\\\", \\\"require\\\": [<paths, or empty>]}}\" \\\n\
         \x20 >/dev/null || true\n\
         ```\n\
         Put in `require` the paths of any output files this run was told it must produce (a \
         JSON array of strings; use `[]` when there are none). brain keeps the tab open until \
         every listed path exists, so a premature signal cannot cut the run short. If \
         `{DONE_URL_ENV}` is unset, skip this step entirely — there is no tab to close.\n\
         Do not signal early, and do not ask the user whether to signal: it is the last thing \
         you do."
    )
}
