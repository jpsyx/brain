//! Restartable rollback renames and authenticated disposal of recorded slots.
use super::{
    journal::{Journal, Phase},
    snapshot::{self, Snapshot},
};
use anyhow::{Result, ensure};
use std::{fs, path::Path};

fn remaining(observed: &Snapshot, expected: &Snapshot) -> bool {
    if *observed == Snapshot::Absent {
        return true;
    }
    match (observed, expected) {
        (
            Snapshot::Directory { mode, entries },
            Snapshot::Directory {
                mode: wanted_mode,
                entries: wanted,
            },
        ) => {
            mode == wanted_mode
                && entries.iter().all(|(name, child)| {
                    wanted
                        .get(name)
                        .is_some_and(|expected| remaining(child, expected))
                })
        }
        _ => observed == expected,
    }
}
impl Phase {
    pub fn disposing(self) -> bool {
        matches!(self, Self::DisposingRollback | Self::DisposingCommitted)
    }
}
impl Journal {
    pub fn transition(&mut self, phase: Phase, cache: &Path) -> Result<()> {
        self.phase = phase;
        self.save(cache)
    }
    pub fn verify_before(&self, root: &Path) -> Result<()> {
        self.verify_sources(root)?;
        for entry in &self.entries {
            snapshot::check_parent(root, &entry.live)?;
            ensure!(
                snapshot::read(&root.join(&entry.live))? == entry.before,
                "package artifact differs from its before-image: {}",
                entry.live
            );
        }
        Ok(())
    }
    pub fn validate_slots(&self, root: &Path) -> Result<()> {
        for (index, entry) in self.entries.iter().enumerate() {
            snapshot::check_parent(root, &entry.live)?;
            for (role, expected) in [
                ("stage", &entry.after),
                ("backup", &entry.before),
                ("discard", &entry.after),
            ] {
                let observed =
                    snapshot::read(&entry.slot(root, &self.transaction_id, index, role))?;
                if (role == "discard"
                    && !matches!(self.phase, Phase::RollingBack | Phase::DisposingRollback))
                    || (role == "backup" && self.phase == Phase::DisposingRollback)
                {
                    ensure!(
                        observed == Snapshot::Absent,
                        "unexpected skills recovery slot"
                    );
                } else if self.phase.disposing() {
                    ensure!(
                        remaining(&observed, expected),
                        "skills disposal content changed"
                    );
                } else {
                    ensure!(
                        observed == Snapshot::Absent || observed == *expected,
                        "skills recovery slot conflict"
                    );
                }
            }
        }
        Ok(())
    }
    pub fn validate_rollback(&self, root: &Path) -> Result<()> {
        self.validate_slots(root)?;
        for (index, entry) in self.entries.iter().enumerate() {
            snapshot::check_parent(root, &entry.live)?;
            let observed = snapshot::read(&root.join(&entry.live))?;
            let backup = snapshot::read(&entry.slot(root, &self.transaction_id, index, "backup"))?;
            let discard =
                snapshot::read(&entry.slot(root, &self.transaction_id, index, "discard"))?;
            if backup != Snapshot::Absent || entry.before == Snapshot::Absent {
                ensure!(
                    observed == entry.after || observed == Snapshot::Absent,
                    "skills recovery conflict at {}",
                    entry.live
                );
                ensure!(
                    observed == Snapshot::Absent || discard == Snapshot::Absent,
                    "skills live and discard both present"
                );
            } else {
                ensure!(
                    observed == entry.before,
                    "skills original missing during recovery: {}",
                    entry.live
                );
            }
        }
        Ok(())
    }
    pub fn rollback(&mut self, root: &Path, cache: &Path) -> Result<()> {
        self.validate_rollback(root)?;
        self.transition(Phase::RollingBack, cache)?;
        for (index, entry) in self.entries.iter().enumerate().rev() {
            snapshot::check_parent(root, &entry.live)?;
            let live = root.join(&entry.live);
            let backup = entry.slot(root, &self.transaction_id, index, "backup");
            let discard = entry.slot(root, &self.transaction_id, index, "discard");
            let has_backup = snapshot::read(&backup)? != Snapshot::Absent;
            if has_backup || entry.before == Snapshot::Absent {
                let observed = snapshot::read(&live)?;
                ensure!(
                    observed == entry.after || observed == Snapshot::Absent,
                    "skills recovery conflict"
                );
                if observed != Snapshot::Absent {
                    ensure!(
                        snapshot::read(&discard)? == Snapshot::Absent,
                        "skills discard collision"
                    );
                    fs::rename(&live, &discard)?;
                    snapshot::sync_dir(live.parent().expect("validated artifact parent"))?;
                }
                if has_backup {
                    ensure!(
                        snapshot::read(&backup)? == entry.before,
                        "skills backup changed"
                    );
                    fs::rename(&backup, &live)?;
                    snapshot::sync_dir(live.parent().expect("validated artifact parent"))?;
                }
            }
        }
        self.verify_before(root)?;
        self.transition(Phase::DisposingRollback, cache)
    }
    pub fn cleanup(&self, root: &Path, cache: &Path) -> Result<()> {
        ensure!(
            self.phase.disposing(),
            "skills disposal intent is not durable"
        );
        if self.phase == Phase::DisposingRollback {
            self.verify_before(root)?;
        } else {
            self.verify_after(root)?;
        }
        self.validate_slots(root)?;
        for (index, entry) in self.entries.iter().enumerate() {
            for (role, expected) in [
                ("stage", &entry.after),
                ("backup", &entry.before),
                ("discard", &entry.after),
            ] {
                snapshot::check_parent(root, &entry.live)?;
                let path = entry.slot(root, &self.transaction_id, index, role);
                let observed = snapshot::read(&path)?;
                ensure!(
                    remaining(&observed, expected),
                    "skills disposal content changed"
                );
                if observed != Snapshot::Absent {
                    snapshot::remove(&path, &observed)?;
                    snapshot::sync_dir(path.parent().expect("validated artifact parent"))?;
                }
            }
        }
        fs::remove_file(cache.join("pending.json"))?;
        snapshot::sync_dir(cache)
    }
}
