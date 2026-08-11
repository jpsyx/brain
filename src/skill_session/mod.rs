//! Skill sessions: single-prompt agent sessions that run in their own
//! brain-panel tab and close themselves when the run is done.
//!
//! A *skill session* dedicates one ephemeral, untracked agent session to one
//! prompt — typically a slash command for a single skill, hence the name, though
//! nothing requires the prompt to be a skill. Daily triage was the first: saying
//! "Yes" to the startup nudge spawns a tab seeded with `/triage` so the (long,
//! often interactive) pass doesn't block the main session. That mechanism is now
//! generic, and a workspace can declare its own sessions in the `skill_sessions`
//! env array (`title`, `prompt`, `command_label`).
//!
//! Every skill session behaves identically:
//!
//! - one tab, one prompt, one session — never resumed, never recorded in the
//!   session DB, so quitting brain mid-run loses it and the user re-runs it;
//! - the tab auto-closes when the run signals completion (see [`signal`]) or the
//!   session exits on its own;
//! - while it runs, its command-palette row disappears, so the same session
//!   can't be started twice.
//!
//! Daily triage is the one **builtin** definition. It is offered only while the
//! workspace's daily-triage check is enabled, and it is neither editable nor
//! removable through `brain env` — unlike every other row, which comes from the
//! workspace's own `skill_sessions` array.
//!
//! This module is the pure half: the model, what the workspace currently
//! offers, and the prompt text. The tab lifecycle lives in
//! `tui::app_skill_session`.

pub mod editor;
pub mod prompt;
pub mod signal;

use serde_json::Value;

/// The per-workspace env variable holding the configured skill sessions.
pub const ENV_VAR: &str = "skill_sessions";

/// Which skill session a tab, palette row, or running controller belongs to.
///
/// `Custom` carries the definition's index in the workspace's `skill_sessions`
/// array — its position, not its position among the *valid* entries, so
/// dropping a malformed sibling can't silently repoint a palette row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkillSessionKey {
    DailyTriage,
    Custom(usize),
}

/// One runnable skill session: what to call it, what to send, and how to offer
/// it in the command palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillSessionSpec {
    pub key: SkillSessionKey,
    /// Tab title in the brain panel.
    pub title: String,
    /// The prompt this session is seeded with.
    pub prompt: String,
    /// The command-palette row that starts it.
    pub command_label: String,
}

impl SkillSessionSpec {
    /// The builtin daily-triage session.
    #[must_use]
    pub fn daily_triage() -> Self {
        Self {
            key: SkillSessionKey::DailyTriage,
            title: "Daily triage".to_owned(),
            prompt: "/triage".to_owned(),
            command_label: "Run daily triage".to_owned(),
        }
    }
}

/// Every skill session the workspace currently offers, builtin first.
///
/// `daily_triage_enabled` gates the builtin; `configured` is the raw
/// `skill_sessions` env value (any non-array shape, including `None`, simply
/// contributes nothing). Pure so both the palette and the tests can ask the
/// question without touching the env store.
#[must_use]
pub fn available(daily_triage_enabled: bool, configured: Option<&Value>) -> Vec<SkillSessionSpec> {
    let mut specs = Vec::new();
    if daily_triage_enabled {
        specs.push(SkillSessionSpec::daily_triage());
    }
    specs.extend(configured.map(parse_configured).unwrap_or_default());
    specs
}

/// The sessions that can be *started* right now: everything offered, minus
/// whatever is already running. This is what hides "Run email triage" from the
/// palette while that session is open.
#[must_use]
pub fn runnable<'a>(
    specs: &'a [SkillSessionSpec],
    running: &[SkillSessionKey],
) -> Vec<&'a SkillSessionSpec> {
    specs
        .iter()
        .filter(|spec| !running.contains(&spec.key))
        .collect()
}

/// Parse the workspace's `skill_sessions` array into specs, dropping entries
/// brain can't run. Pure; also what the interactive editor lists.
///
/// `prompt` is the only required field: a session with nothing to send is not a
/// session. `title` falls back to the prompt and `command_label` to
/// `Run <title>`, so the shortest useful definition is one key.
#[must_use]
pub fn parse_configured(configured: &Value) -> Vec<SkillSessionSpec> {
    let Some(entries) = configured.as_array() else {
        return Vec::new();
    };
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| parse_entry(index, entry))
        .collect()
}

fn parse_entry(index: usize, entry: &Value) -> Option<SkillSessionSpec> {
    let prompt = trimmed_field(entry, "prompt")?;
    let title = trimmed_field(entry, "title").unwrap_or_else(|| prompt.clone());
    let command_label =
        trimmed_field(entry, "command_label").unwrap_or_else(|| format!("Run {title}"));
    Some(SkillSessionSpec {
        key: SkillSessionKey::Custom(index),
        title,
        prompt,
        command_label,
    })
}

fn trimmed_field(entry: &Value, name: &str) -> Option<String> {
    let value = entry.get(name)?.as_str()?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests;
