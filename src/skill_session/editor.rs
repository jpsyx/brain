//! `brain env set skill_sessions` with no value: the guided add / edit / delete
//! walkthrough.
//!
//! Every field is settable non-interactively —
//! `brain env set skill_sessions '[{"title":…,"prompt":…,"command_label":…}]'`
//! writes the whole array, and `brain env set skill_sessions.0.prompt=…` one
//! field — so an agent never needs this. A human typing JSON by hand does, which
//! is why the valueless form walks them through it instead of erroring.
//!
//! The list arithmetic (render, add, replace, delete) is pure and unit-tested;
//! only [`run`] touches the terminal and the env store.

use std::fmt::Write as _;

use anyhow::Result;
use serde_json::{Value, json};

use super::{SkillSessionSpec, parse_configured};
use crate::theme::Theme;

/// One editor action, resolved from a keystroke. Pure so the loop's routing is
/// testable without a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorChoice {
    Add,
    Edit(usize),
    Delete(usize),
    Quit,
}

/// Route a menu keystroke against a list of `count` sessions.
///
/// `a` adds, `d<n>` / `e<n>` are handled by their own prompts, a bare row number
/// edits that row, and anything else quits — the safe default for a walkthrough
/// nobody wants to be trapped in.
#[must_use]
pub fn choose(input: &str, count: usize) -> EditorChoice {
    let input = input.trim().to_lowercase();
    match input.as_str() {
        "a" | "add" => EditorChoice::Add,
        "" | "q" | "quit" => EditorChoice::Quit,
        other => {
            let (verb, rest) = other.split_at(1);
            let row = rest.trim().parse::<usize>().ok();
            match (verb, row.and_then(|row| row.checked_sub(1))) {
                ("d", Some(index)) if index < count => EditorChoice::Delete(index),
                ("e", Some(index)) if index < count => EditorChoice::Edit(index),
                _ => match other.parse::<usize>().ok().and_then(|row| row.checked_sub(1)) {
                    Some(index) if index < count => EditorChoice::Edit(index),
                    _ => EditorChoice::Quit,
                },
            }
        }
    }
}

/// The themed listing of a workspace's configured skill sessions.
#[must_use]
pub fn render_list(specs: &[SkillSessionSpec], theme: Theme) -> String {
    if specs.is_empty() {
        return format!(
            "  {}\n",
            theme.muted("No skill sessions configured yet — daily triage is builtin.")
        );
    }
    let mut out = String::new();
    for (row, spec) in specs.iter().enumerate() {
        let _ = write!(
            out,
            "  {}) {}\n     {} {}\n     {} {}\n",
            row + 1,
            theme.accent(&spec.title),
            theme.muted("prompt:"),
            theme.value(&spec.prompt),
            theme.muted("palette:"),
            theme.value(&spec.command_label),
        );
    }
    out
}

/// One session as it is stored: only the fields the user actually gave, so a
/// borrowed default stays a default rather than being frozen into the store.
#[must_use]
pub fn entry_value(title: &str, prompt: &str, command_label: &str) -> Value {
    let mut entry = json!({ "prompt": prompt.trim() });
    let object = entry.as_object_mut().expect("entry is an object");
    if !title.trim().is_empty() {
        object.insert("title".to_owned(), Value::from(title.trim()));
    }
    if !command_label.trim().is_empty() {
        object.insert("command_label".to_owned(), Value::from(command_label.trim()));
    }
    entry
}

/// The array to store after adding `entry` to `current`. Pure.
#[must_use]
pub fn added(current: &Value, entry: Value) -> Value {
    let mut entries = current.as_array().cloned().unwrap_or_default();
    entries.push(entry);
    Value::Array(entries)
}

/// The array to store after replacing row `index` with `entry`. Out-of-range
/// leaves the list unchanged. Pure.
#[must_use]
pub fn replaced(current: &Value, index: usize, entry: Value) -> Value {
    let mut entries = current.as_array().cloned().unwrap_or_default();
    if index < entries.len() {
        entries[index] = entry;
    }
    Value::Array(entries)
}

/// The array to store after deleting row `index`. Out-of-range leaves the list
/// unchanged. Pure.
#[must_use]
pub fn deleted(current: &Value, index: usize) -> Value {
    let mut entries = current.as_array().cloned().unwrap_or_default();
    if index < entries.len() {
        entries.remove(index);
    }
    Value::Array(entries)
}

/// Walk the user through editing the selected workspace's skill sessions.
///
/// # Errors
/// Propagates env-store write failures, and reports when there is no terminal to
/// prompt on (with the non-interactive command to use instead).
pub fn run(context: &crate::workspace::CommandContext) -> Result<()> {
    let theme = Theme::active();
    loop {
        let current = crate::env::get_raw(context, super::ENV_VAR).unwrap_or(Value::Array(vec![]));
        let specs = parse_configured(&current);
        println!(
            "{}",
            theme.heading(&format!(
                "Skill sessions for workspace `{}`",
                context.workspace.name()
            ))
        );
        print!("{}", render_list(&specs, theme));
        println!(
            "  {}",
            theme.muted("Each runs in its own brain-panel tab and closes when its run finishes.")
        );
        let Some(input) = prompt(&theme.prompt("[a]dd  [e]<n> edit  [d]<n> delete  [q]uit: "))?
        else {
            anyhow::bail!(
                "no terminal for the skill-session editor; use `{}`",
                crate::workspace::suggest(
                    "env set skill_sessions '[{\"title\":\"…\",\"prompt\":\"/…\",\"command_label\":\"Run …\"}]'"
                )
            );
        };
        match choose(&input, specs.len()) {
            EditorChoice::Quit => return Ok(()),
            EditorChoice::Add => {
                let Some(entry) = prompt_entry(theme, None)? else {
                    return Ok(());
                };
                store(context, added(&current, entry))?;
                println!("{}", theme.success("✓ skill session added"));
            }
            EditorChoice::Edit(index) => {
                let Some(entry) = prompt_entry(theme, specs.get(index))? else {
                    return Ok(());
                };
                store(context, replaced(&current, index, entry))?;
                println!("{}", theme.success("✓ skill session updated"));
            }
            EditorChoice::Delete(index) => {
                store(context, deleted(&current, index))?;
                println!("{}", theme.success("✓ skill session removed"));
            }
        }
    }
}

fn store(context: &crate::workspace::CommandContext, value: Value) -> Result<()> {
    crate::env::set_raw(context, super::ENV_VAR, value)
}

/// Prompt for one session's three fields, pre-filled from `existing` when
/// editing. `None` means the user (or a missing terminal) ended the walkthrough.
fn prompt_entry(theme: Theme, existing: Option<&SkillSessionSpec>) -> Result<Option<Value>> {
    let ask = |label: &str, current: Option<&str>| -> Result<Option<String>> {
        let suffix = current.map_or_else(String::new, |value| format!(" [{value}]"));
        let answer = prompt(&theme.prompt(&format!("  {label}{suffix}: ")))?;
        Ok(answer.map(|answer| {
            let answer = answer.trim().to_owned();
            if answer.is_empty() {
                current.unwrap_or_default().to_owned()
            } else {
                answer
            }
        }))
    };
    let Some(prompt_text) = ask("Prompt (e.g. /email-triage)", existing.map(|s| s.prompt.as_str()))?
    else {
        return Ok(None);
    };
    if prompt_text.trim().is_empty() {
        println!(
            "{}",
            theme.warning("a skill session needs a prompt; nothing saved")
        );
        return Ok(None);
    }
    let Some(title) = ask("Tab title", existing.map(|s| s.title.as_str()))? else {
        return Ok(None);
    };
    let Some(label) = ask(
        "Command-palette label",
        existing.map(|s| s.command_label.as_str()),
    )?
    else {
        return Ok(None);
    };
    Ok(Some(entry_value(&title, &prompt_text, &label)))
}

fn prompt(label: &str) -> Result<Option<String>> {
    crate::command::prompt_tty_line(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(title: &str) -> SkillSessionSpec {
        SkillSessionSpec {
            key: super::super::SkillSessionKey::Custom(0),
            title: title.to_owned(),
            prompt: "/email-triage".to_owned(),
            command_label: "Run email triage".to_owned(),
        }
    }

    #[test]
    fn menu_keys_route_to_their_actions() {
        assert_eq!(choose("a", 2), EditorChoice::Add);
        assert_eq!(choose("  A  ", 2), EditorChoice::Add);
        assert_eq!(choose("e2", 2), EditorChoice::Edit(1));
        assert_eq!(choose("d1", 2), EditorChoice::Delete(0));
        // A bare row number is the common case: edit that row.
        assert_eq!(choose("2", 2), EditorChoice::Edit(1));
        assert_eq!(choose("q", 2), EditorChoice::Quit);
    }

    #[test]
    fn a_row_that_does_not_exist_quits_rather_than_editing_a_neighbor() {
        assert_eq!(choose("e9", 2), EditorChoice::Quit);
        assert_eq!(choose("d3", 2), EditorChoice::Quit);
        assert_eq!(choose("0", 2), EditorChoice::Quit);
        assert_eq!(choose("", 2), EditorChoice::Quit);
        assert_eq!(choose("nonsense", 2), EditorChoice::Quit);
    }

    #[test]
    fn an_entry_stores_only_the_fields_the_user_supplied() {
        let minimal = entry_value("", "/email-triage", "");
        assert_eq!(minimal, json!({"prompt": "/email-triage"}));

        let full = entry_value("Email triage", " /email-triage ", "Run email triage");
        assert_eq!(
            full,
            json!({
                "prompt": "/email-triage",
                "title": "Email triage",
                "command_label": "Run email triage",
            })
        );
    }

    #[test]
    fn add_edit_and_delete_rewrite_the_stored_array() {
        let empty = Value::Array(vec![]);
        let one = added(&empty, entry_value("Email triage", "/email-triage", ""));
        assert_eq!(one.as_array().map(Vec::len), Some(1));

        let renamed = replaced(&one, 0, entry_value("Inbox", "/email-triage", ""));
        assert_eq!(renamed[0]["title"], json!("Inbox"));

        assert!(deleted(&renamed, 0).as_array().unwrap().is_empty());
        // Out-of-range edits leave the list alone rather than growing it.
        assert_eq!(replaced(&one, 5, entry_value("x", "/x", "")), one);
        assert_eq!(deleted(&one, 5), one);
    }

    #[test]
    fn a_missing_list_renders_as_the_builtin_only_note() {
        let rendered = render_list(&[], Theme::dark(false));
        assert!(rendered.contains("daily triage is builtin"), "{rendered}");
    }

    #[test]
    fn the_listing_shows_each_sessions_prompt_and_palette_label() {
        let rendered = render_list(&[spec("Email triage")], Theme::dark(false));
        assert!(rendered.contains("Email triage"), "{rendered}");
        assert!(rendered.contains("/email-triage"), "{rendered}");
        assert!(rendered.contains("Run email triage"), "{rendered}");
    }
}
