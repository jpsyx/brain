//! rclone `--check-access` marker lifecycle.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::sync::remote::Remote;

/// rclone's default check-access marker filename, made explicit in argv.
pub const CHECK_FILENAME: &str = "RCLONE_TEST";

const MARKER_CONTENT: &str = "brain sync access marker\n";

/// The local marker path under the brain root.
#[must_use]
pub fn marker_path(root: &Path) -> PathBuf {
    root.join(CHECK_FILENAME)
}

/// The remote marker path under the configured remote root.
#[must_use]
pub fn remote_marker_arg(remote_root: &str) -> String {
    format!("{}/{}", remote_root.trim_end_matches('/'), CHECK_FILENAME)
}

/// Ensure the local marker exists with generic content.
pub fn ensure_local_marker(root: &Path) -> Result<()> {
    std::fs::write(marker_path(root), MARKER_CONTENT)?;
    Ok(())
}

/// Ensure both local and remote check-access markers exist.
pub fn ensure_markers(root: &Path, remote: &Remote) -> Result<()> {
    ensure_markers_with(root, remote, crate::sync::run::run_rclone_capture)
}

/// Ensure both local and remote check-access markers exist, with rclone injected
/// for tests.
pub fn ensure_markers_with(
    root: &Path,
    remote: &Remote,
    run: impl FnMut(&[(String, String)], &[String]) -> (bool, String),
) -> Result<()> {
    ensure_local_marker(root)?;
    ensure_remote_marker_with(root, remote, run)
}

/// Ensure the remote marker exists by copying the local marker to the remote root.
pub fn ensure_remote_marker_with(
    root: &Path,
    remote: &Remote,
    mut run: impl FnMut(&[(String, String)], &[String]) -> (bool, String),
) -> Result<()> {
    let args = vec![
        "copyto".to_owned(),
        marker_path(root).display().to_string(),
        remote_marker_arg(&remote.arg),
    ];
    let (ok, output) = run(&remote.env, &args);
    if ok {
        Ok(())
    } else {
        anyhow::bail!("could not create remote check-access marker: {output}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_path_lives_at_the_brain_root() {
        assert_eq!(marker_path(Path::new("/brain")), PathBuf::from("/brain/RCLONE_TEST"));
    }

    #[test]
    fn remote_marker_arg_trims_remote_root_slash() {
        assert_eq!(remote_marker_arg("BRAIN:bucket/prefix/"), "BRAIN:bucket/prefix/RCLONE_TEST");
    }

    #[test]
    fn ensure_local_marker_writes_generic_content() {
        let tmp = std::env::temp_dir().join(format!("brain-check-access-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        ensure_local_marker(&tmp).unwrap();

        assert_eq!(
            std::fs::read_to_string(tmp.join(CHECK_FILENAME)).unwrap(),
            MARKER_CONTENT
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn ensure_remote_marker_copies_local_marker_to_remote_marker() {
        let tmp = std::env::temp_dir().join(format!("brain-check-access-remote-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        ensure_local_marker(&tmp).unwrap();
        let remote = Remote {
            env: vec![("RCLONE_CONFIG_BRAIN_TYPE".to_owned(), "b2".to_owned())],
            arg: "BRAIN:bucket/prefix".to_owned(),
        };

        let mut seen_env = Vec::new();
        let mut seen_args = Vec::new();
        ensure_remote_marker_with(&tmp, &remote, |env, args| {
            seen_env = env.to_vec();
            seen_args = args.to_vec();
            (true, String::new())
        })
        .unwrap();

        assert_eq!(seen_env, remote.env);
        assert_eq!(
            seen_args,
            vec![
                "copyto".to_owned(),
                tmp.join(CHECK_FILENAME).display().to_string(),
                "BRAIN:bucket/prefix/RCLONE_TEST".to_owned(),
            ]
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn ensure_markers_writes_local_marker_before_remote_copy() {
        let tmp = std::env::temp_dir().join(format!("brain-check-access-both-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let remote = Remote { env: Vec::new(), arg: "BRAIN:bucket".to_owned() };

        ensure_markers_with(&tmp, &remote, |_, args| {
            assert!(
                tmp.join(CHECK_FILENAME).exists(),
                "local marker must exist before the remote copy runs"
            );
            assert_eq!(args[2], "BRAIN:bucket/RCLONE_TEST");
            (true, String::new())
        })
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(tmp.join(CHECK_FILENAME)).unwrap(),
            MARKER_CONTENT
        );

        std::fs::remove_dir_all(&tmp).ok();
    }
}
