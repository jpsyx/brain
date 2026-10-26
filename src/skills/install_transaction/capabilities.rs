//! Immutable selected outputs, published under the existing installation lock.
use super::{
    LockedInstall, RenderedInstall, ownership,
    snapshot::{self, Snapshot},
    validation,
};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt as _, path::PathBuf};

impl LockedInstall {
    pub(crate) fn publish_capabilities(
        &self,
        actor: &crate::users::UserId,
        rendered: &RenderedInstall,
    ) -> Result<PathBuf> {
        let encoded = serde_json::to_vec(rendered)?;
        snapshot::check_state_bytes(&encoded)?;
        let key = snapshot::digest(&encoded);
        let parent = self
            .identity
            .cache
            .parent()
            .expect("skills cache parent")
            .join("skill-generations/actors")
            .join(actor.to_string());
        snapshot::ensure_absolute_directory(&parent)?;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))?;
        let destination = parent.join(key);
        let mut desired = Snapshot::Directory {
            mode: 0o40700,
            entries: BTreeMap::new(),
        };
        let mut files = BTreeMap::new();
        for (name, contents) in rendered {
            validation::name(name)?;
            ensure!(
                contents.contains_key("SKILL.md"),
                "selected skill has no SKILL.md"
            );
            for (relative, bytes) in contents {
                snapshot::relative(relative)?;
                let path = format!("{name}/{relative}");
                ownership::insert(
                    &mut desired,
                    &path,
                    Snapshot::File {
                        sha256: snapshot::digest(bytes),
                        mode: 0o100_600,
                    },
                )?;
                files.insert(format!("skills/{path}"), bytes.clone());
            }
        }
        ensure!(files.len() <= 100_000, "oversized selected skill inventory");
        let existing = snapshot::read(&destination)?;
        if existing == desired {
            return Ok(destination);
        }
        ensure!(
            existing == Snapshot::Absent,
            "retained selected skill generation has changed"
        );
        let staged = parent.join(format!(".pending-{}", uuid::Uuid::new_v4()));
        snapshot::materialize(&parent, &staged, "skills", &desired, &files)?;
        ensure!(
            snapshot::read(&destination)? == Snapshot::Absent,
            "selected skill generation appeared during publication"
        );
        fs::rename(staged, &destination)?;
        snapshot::sync_dir(&parent)?;
        ensure!(
            snapshot::read(&destination)? == desired,
            "selected skill publication verification failed"
        );
        Ok(destination)
    }
}
