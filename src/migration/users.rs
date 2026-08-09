//! Pre-mutation portable-user preparation and interactive mapping.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::mapping_prompt::{
    MappingChoice, interpret_choice, mapping_options, mapping_question, proposed_new_id,
};
use super::{
    MappingIssue, MappingResolution, apply_mapping_resolution, headless_mapping_remediation,
    mapping_issues,
};
use crate::config::Config;
use crate::users::{
    AssignmentRewrites, UserId, UserMutation, Users, UsersStore, apply_mutation,
    propose_legacy_user_migration,
};
use crate::workspace::CommandContext;

pub(super) struct PreparedUsers {
    users: Users,
    original: Option<Vec<u8>>,
    rewrites: AssignmentRewrites,
}

impl PreparedUsers {
    pub(super) const fn assignment_rewrites(&self) -> &AssignmentRewrites {
        &self.rewrites
    }

    pub(super) fn persist(&self, context: &CommandContext) -> Result<()> {
        let path = UsersStore::path(&context.workspace);
        let observed = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).context("rechecking portable users before migration write");
            }
        };
        if observed != self.original {
            if observed.as_deref() == Some(self.users.to_bytes()?.as_slice()) {
                return Ok(());
            }
            bail!("portable users changed after migration preflight; rerun migration");
        }
        UsersStore::save(&context.workspace, &self.users)?;
        Ok(())
    }
}

pub(super) fn prepare(
    context: &CommandContext,
    config: &Config,
    terminal: Option<&mut Terminal>,
) -> Result<PreparedUsers> {
    let assignments = read_assignments(context.workspace.root())?;
    let path = UsersStore::path(&context.workspace);
    let original = fs::read(&path).ok();
    let mut terminal = terminal;
    let mut users = match UsersStore::load(&context.workspace) {
        Ok(users) => users,
        Err(error) if error.is_missing_store() => {
            let mut name = crate::personalization::store::local_persona(&context.workspace).name;
            if name.trim().is_empty() {
                let Some(terminal) = terminal.as_deref_mut() else {
                    bail!(
                        "portable users are missing; run `brain user add -w {} --id {} --name <DISPLAY_NAME>` then `brain user local {} -w {}`",
                        context.workspace.name().as_str(),
                        context.workspace.local_user_id(),
                        context.workspace.local_user_id(),
                        context.workspace.name().as_str()
                    );
                };
                name = terminal.required("Display name for the local portable user:")?;
            }
            let proposal = propose_legacy_user_migration(
                &name,
                Some(context.workspace.local_user_id()),
                &config.response_email,
                &config.allowed_sms(),
                &config.allowed_email(),
            )?;
            let mut users = Users::empty();
            apply_mutation(&mut users, UserMutation::Add(proposal.user))?;
            users
        }
        Err(error) => return Err(error.into()),
    };

    let mut rewrites = AssignmentRewrites::new();
    let issues = mapping_issues(&users, config, &assignments);
    if !issues.is_empty() {
        let Some(terminal) = terminal else {
            bail!(
                "legacy identities require portable user mappings before migration:\n{}",
                headless_mapping_remediation(context.workspace.name().as_str(), &issues)
            );
        };
        for issue in &issues {
            let resolution = terminal.resolve(issue, &users)?;
            apply_mapping_resolution(&mut users, &mut rewrites, issue, resolution)?;
        }
    }
    let resolved = assignments
        .iter()
        .map(|value| rewrites.apply(value).to_owned())
        .collect::<Vec<_>>();
    if !mapping_issues(&users, config, &resolved).is_empty() {
        bail!("legacy identity mapping remained incomplete");
    }
    Ok(PreparedUsers {
        users,
        original,
        rewrites,
    })
}

pub(super) struct Terminal {
    reader: BufReader<File>,
    writer: File,
}

impl Terminal {
    pub(super) fn open() -> Result<Self> {
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("open /dev/tty for workspace migration")?;
        let reader = BufReader::new(writer.try_clone().context("clone migration terminal")?);
        Ok(Self { reader, writer })
    }

    pub(super) fn confirm_all_machines_updated(&mut self) -> Result<()> {
        loop {
            let answer = self.required(
                "Every machine syncing this workspace must run a migration-capable Brain version. Continue? [y/N]",
            )?;
            match answer.to_ascii_lowercase().as_str() {
                "y" | "yes" => return Ok(()),
                "n" | "no" => bail!("workspace migration cancelled before portable mutation"),
                _ => writeln!(self.writer, "Please answer yes or no.")?,
            }
        }
    }

    fn resolve(&mut self, issue: &MappingIssue, users: &Users) -> Result<MappingResolution> {
        let theme = crate::theme::Theme::active();
        writeln!(self.writer, "{}", theme.heading(&mapping_question(issue)))?;
        for option in mapping_options(issue, users) {
            writeln!(self.writer, "  {}", theme.value(&option))?;
        }
        let choice = loop {
            let answer = self.required("Choose a person by number or ID:")?;
            if let Some(choice) = interpret_choice(users, &answer) {
                break choice;
            }
            writeln!(
                self.writer,
                "{}",
                theme.warning("Answer with one of the numbers above or an existing user ID.")
            )?;
        };
        match choice {
            MappingChoice::Existing(id) => Ok(MappingResolution::Existing(id)),
            MappingChoice::CreateNew => {
                let id = match proposed_new_id(issue) {
                    Some(id) => id,
                    None => self.new_user_id()?,
                };
                let name = self.required(&format!("Display name for {id}:"))?;
                Ok(MappingResolution::New { id, name })
            }
        }
    }

    fn new_user_id(&mut self) -> Result<UserId> {
        let theme = crate::theme::Theme::active();
        loop {
            let value = self.required("New user ID (lower-case kebab case):")?;
            match UserId::parse(&value) {
                Ok(id) => return Ok(id),
                Err(error) => writeln!(self.writer, "{}", theme.warning(&error.to_string()))?,
            }
        }
    }

    fn required(&mut self, label: &str) -> Result<String> {
        loop {
            write!(
                self.writer,
                "{} ",
                crate::theme::Theme::active().prompt(label)
            )?;
            self.writer.flush()?;
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                bail!("workspace migration cancelled before portable mutation");
            }
            let value = line.trim();
            if !value.is_empty() {
                return Ok(value.to_owned());
            }
        }
    }
}

fn read_assignments(root: &Path) -> Result<Vec<String>> {
    let mut assignments = Vec::new();
    for name in ["tasks.csv", "habits.csv"] {
        let path = root.join("tasks").join(name);
        let mut reader = csv::Reader::from_path(&path)
            .with_context(|| format!("reading assignments from {}", path.display()))?;
        let headers = reader.headers()?.clone();
        let index = headers
            .iter()
            .position(|header| header == "assigned_to")
            .or_else(|| headers.iter().position(|header| header == "assignee"));
        if let Some(index) = index {
            for row in reader.records() {
                let row = row?;
                let value = row.get(index).unwrap_or_default().trim();
                if !value.is_empty() {
                    assignments.push(value.to_owned());
                }
            }
        }
    }
    Ok(assignments)
}

#[cfg(test)]
mod tests {
    use super::read_assignments;

    #[test]
    fn mapping_preflight_prefers_canonical_assigned_to_over_legacy_assignee() {
        let temporary = tempfile::tempdir().unwrap();
        let tasks = temporary.path().join("tasks");
        std::fs::create_dir(&tasks).unwrap();
        std::fs::write(
            tasks.join("tasks.csv"),
            "task_id,assignee,assigned_to\nT1,legacy-person,canonical-person\n",
        )
        .unwrap();
        std::fs::write(
            tasks.join("habits.csv"),
            "task_id,assigned_to,assignee\nH1,canonical-habit,legacy-habit\n",
        )
        .unwrap();

        assert_eq!(
            read_assignments(temporary.path()).unwrap(),
            ["canonical-person", "canonical-habit"]
        );
    }
}
