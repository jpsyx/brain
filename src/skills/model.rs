//! The in-memory representation of a skill to install, shared by the embedded
//! bundled skills (`embed`) and the user's on-disk plugins (`plugin`).

use std::path::PathBuf;

/// A skill and every file under it (paths relative to the skill dir).
pub struct Skill {
    pub name: String,
    pub files: Vec<SkillFile>,
}

pub struct SkillFile {
    pub rel_path: PathBuf,
    pub contents: Vec<u8>,
}
