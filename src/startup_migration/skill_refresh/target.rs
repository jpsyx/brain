//! Byte-bound trusted target invocation in a private process environment.
use crate::cli::{RefreshPhase, RefreshWorkspaceSkillsArgs};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

pub(super) struct PrivateDirectory(PathBuf);
impl PrivateDirectory {
    pub(super) fn new() -> Result<Self> {
        let path =
            std::env::temp_dir().join(format!("brain-skill-refresh-{}", uuid::Uuid::new_v4()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        let directory = Self(path.canonicalize()?);
        for name in [
            "home",
            "xdg-config",
            "xdg-cache",
            "xdg-data",
            "xdg-runtime",
            "tmp",
            "workspace",
        ] {
            builder.create(directory.path().join(name))?;
        }
        Ok(directory)
    }
    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub(super) struct Target {
    pub(super) path: PathBuf,
    digest: String,
    pub(super) version: String,
    pub(super) profile: Profile,
    pub(super) legacy_evidence: bool,
}
impl Target {
    pub(super) fn validate(args: &RefreshWorkspaceSkillsArgs) -> Result<Self> {
        ensure!(
            args.phase != RefreshPhase::Apply || args.expected_target_sha256.is_some(),
            "apply requires the modern installer expected target digest"
        );
        if let Some(digest) = &args.expected_target_sha256 {
            ensure!(
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "invalid expected target SHA-256"
            );
        }
        let digest = digest(&args.target_binary)?;
        ensure!(
            args.expected_target_sha256
                .as_ref()
                .is_none_or(|expected| expected == &digest),
            "target executable digest changed"
        );
        let profile = Profile::for_version(&args.target_version)?;
        let target = Self {
            path: args.target_binary.clone(),
            digest,
            version: args.target_version.clone(),
            profile,
            legacy_evidence: args
                .from_version
                .as_deref()
                .map(crate::startup_migration::brain_skills::predates_split)
                .transpose()?
                .unwrap_or(false),
        };
        let private = PrivateDirectory::new()?;
        let version = target.run(&private, &["--version"])?;
        ensure!(
            String::from_utf8(version.stdout)?.trim() == format!("brain {}", target.version),
            "target reported a different version"
        );
        let help = target.run(&private, &["--help"])?;
        let help = String::from_utf8(help.stdout)?;
        ensure!(
            help.contains("--workspace") && help.contains("-w"),
            "target lacks selected workspace grammar"
        );
        let help = target.run(&private, &["skills", "--help"])?;
        ensure!(
            String::from_utf8(help.stdout)?.contains("sync"),
            "target lacks skill sync grammar"
        );
        let help = target.run(
            &private,
            &["-w", "refresh-probe", "skills", "sync", "--help"],
        )?;
        ensure!(
            String::from_utf8(help.stdout)?.contains("sync"),
            "target lacks selected sync help"
        );
        Ok(target)
    }
    pub(super) fn verify(&self) -> Result<()> {
        ensure!(
            digest(&self.path)? == self.digest,
            "target executable digest changed"
        );
        Ok(())
    }
    pub(super) fn run(&self, private: &PrivateDirectory, args: &[&str]) -> Result<Output> {
        self.verify()?;
        let base = private.path();
        let output = Command::new(&self.path)
            .args(args)
            .env_clear()
            .env("HOME", base.join("home"))
            .env("XDG_CONFIG_HOME", base.join("xdg-config"))
            .env("XDG_CACHE_HOME", base.join("xdg-cache"))
            .env("XDG_DATA_HOME", base.join("xdg-data"))
            .env("XDG_RUNTIME_DIR", base.join("xdg-runtime"))
            .env("TMPDIR", base.join("tmp"))
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("NO_COLOR", "1")
            .current_dir(base.join("workspace"))
            .stdin(Stdio::null())
            .output()?;
        self.verify()?;
        ensure!(
            output.status.success(),
            "target skill command failed (child diagnostics withheld to protect source content)"
        );
        Ok(output)
    }
}
pub(super) fn digest(path: &Path) -> Result<String> {
    ensure!(
        path.is_absolute(),
        "target executable path must be absolute"
    );
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "target executable must be a real regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        ensure!(
            metadata.permissions().mode() & 0o111 != 0,
            "target is not executable"
        );
    }
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Exact audited semantics, independent of the installed coordinator's version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Profile {
    Legacy101,
    Legacy105,
    Current,
}
impl Profile {
    fn for_version(version: &str) -> Result<Self> {
        match version {
            "0.101.0" => Ok(Self::Legacy101),
            "0.105.3" => Ok(Self::Legacy105),
            env!("CARGO_PKG_VERSION") => Ok(Self::Current),
            _ => anyhow::bail!("target version has no audited skill refresh profile"),
        }
    }
    pub(super) const fn revision(self) -> u32 {
        match self {
            Self::Legacy101 => 0,
            Self::Legacy105 => 1,
            Self::Current => 2,
        }
    }
    pub(super) const fn schema(self) -> u32 {
        match self {
            Self::Legacy101 | Self::Legacy105 => 1,
            Self::Current => 2,
        }
    }
    pub(super) const fn task_schema(self) -> u64 {
        match self {
            Self::Legacy101 | Self::Legacy105 => 2,
            Self::Current => crate::tasks::schema::TASK_SCHEMA_VERSION,
        }
    }
    pub(super) const fn direction(self) -> crate::skills::compatibility::ProjectionDirection {
        use crate::skills::compatibility::ProjectionDirection;
        match self {
            Self::Legacy101 => ProjectionDirection::Legacy101,
            Self::Legacy105 => ProjectionDirection::Legacy105,
            Self::Current => ProjectionDirection::Current,
        }
    }
}
