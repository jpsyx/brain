//! Fixed policy choices and explicit consent, only entered by terminal callers.
use crate::tasks::cli::RemoveArgs;
use crate::tasks::removal::model::{Dependents, LinkedIssue, Options, Problem, RemovalPlan};
use std::io::{BufRead, BufReader, Write};

pub(super) fn choose_policies(
    plan: &RemovalPlan,
    args: &RemoveArgs,
    options: &mut Options,
) -> Result<bool, Problem> {
    with_tty(|reader, writer| choose_from(plan, args, options, reader, writer))
}
pub(super) fn confirm() -> Result<bool, Problem> {
    with_tty(confirm_from)
}
fn with_tty<T>(
    action: impl FnOnce(&mut BufReader<std::fs::File>, &mut std::fs::File) -> std::io::Result<T>,
) -> Result<T, Problem> {
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| super::prompt_failed())?;
    let mut writer = tty.try_clone().map_err(|_| super::prompt_failed())?;
    action(&mut BufReader::new(tty), &mut writer).map_err(|_| super::prompt_failed())
}
fn choose_from(
    plan: &RemovalPlan,
    args: &RemoveArgs,
    options: &mut Options,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> std::io::Result<bool> {
    let mut changed = false;
    if args.dependents.is_none() && plan.blockers.iter().any(|p| p.code == "dependents_exist") {
        match choice(
            reader,
            writer,
            "Dependents: [1] reject, [2] unlink, [3] rewire (Enter cancels): ",
        )?
        .as_deref()
        {
            Some("2" | "unlink") => {
                options.dependents = Dependents::Unlink;
                changed = true;
            }
            Some("3" | "rewire") => {
                options.dependents = Dependents::Rewire;
                changed = true;
            }
            _ => return Ok(false),
        }
    }
    if args.linked_issue.is_none()
        && plan.blockers.iter().any(|p| {
            matches!(
                p.code.as_str(),
                "adapter_unavailable" | "shared_external_link"
            )
        })
    {
        match choice(
            reader,
            writer,
            "Linked issue: [1] configured, [2] keep, [3] cancel (Enter cancels): ",
        )?
        .as_deref()
        {
            Some("2" | "keep") => {
                changed |= options.linked_issue != LinkedIssue::Keep;
                options.linked_issue = LinkedIssue::Keep;
            }
            Some("3" | "cancel") => {
                changed |= options.linked_issue != LinkedIssue::Cancel;
                options.linked_issue = LinkedIssue::Cancel;
            }
            _ => return Ok(false),
        }
    }
    Ok(changed)
}
fn confirm_from(reader: &mut impl BufRead, writer: &mut impl Write) -> std::io::Result<bool> {
    Ok(choice(
        reader,
        writer,
        "Remove these targets and apply these effects? Type yes to confirm: ",
    )?
    .is_some_and(|s| s.eq_ignore_ascii_case("yes")))
}
fn choice(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    prompt: &str,
) -> std::io::Result<Option<String>> {
    write!(writer, "{}", crate::theme::Theme::active().prompt(prompt))?;
    writer.flush()?;
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim().to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Cursor;
    #[test]
    fn removal_confirmation_requires_full_yes_and_eof_cancels() {
        for (input, expected) in [
            ("yes\n", true),
            ("YES\n", true),
            ("y\n", false),
            ("\n", false),
            ("", false),
        ] {
            let mut output = Vec::new();
            assert_eq!(
                confirm_from(&mut Cursor::new(input), &mut output).unwrap(),
                expected
            );
            assert!(String::from_utf8(output).unwrap().contains("Type yes"));
        }
    }
    #[test]
    fn removal_fixed_policy_choices_never_override_explicit_flags() {
        let parsed = crate::tasks::cli::Cli::try_parse_from([
            "tasks",
            "remove",
            "T1",
            "--dependents",
            "reject",
        ])
        .unwrap();
        let Some(crate::tasks::cli::Command::Remove(args)) = parsed.command else {
            panic!("remove")
        };
        let mut options = Options::default();
        let mut plan = RemovalPlan {
            fingerprint: "hash".into(),
            targets: vec![],
            options: options.clone(),
            edits: vec![],
            external: vec![],
            successors: std::collections::BTreeMap::new(),
            warnings: vec![],
            blockers: vec![Problem::new(
                "dependents_exist",
                "dependent",
                serde_json::json!({}),
            )],
        };
        let mut reader = Cursor::new("2\n");
        let mut output = Vec::new();
        assert!(!choose_from(&plan, &args, &mut options, &mut reader, &mut output).unwrap());
        assert!(output.is_empty());
        assert_eq!(reader.position(), 0);
        let parsed = crate::tasks::cli::Cli::try_parse_from(["tasks", "remove", "T1"]).unwrap();
        let Some(crate::tasks::cli::Command::Remove(args)) = parsed.command else {
            panic!("remove")
        };
        assert!(choose_from(&plan, &args, &mut options, &mut reader, &mut output).unwrap());
        assert_eq!(options.dependents, Dependents::Unlink);
        plan.blockers[0].code = "adapter_unavailable".into();
        options.linked_issue = LinkedIssue::Cancel;
        assert!(
            !choose_from(
                &plan,
                &args,
                &mut options,
                &mut Cursor::new("3\n"),
                &mut output
            )
            .unwrap()
        );
    }
}
