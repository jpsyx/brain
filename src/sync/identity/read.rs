//! The one rule for reading a single remote file, shared by every identity lane.
//!
//! `rclone cat` of a missing object exits 0 with no output on some backends (B2
//! among them), so the exit status alone never answers whether the file is
//! there. Only real bytes claim anything. Both the ownership claim and the
//! portable manifest learned this the hard way; they now share one reader.

use super::RemoteCommandOutput;

pub(super) enum RemoteRead {
    Present(Vec<u8>),
    Absent { stderr: String },
}

impl RemoteRead {
    pub(super) fn bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Present(bytes) => Some(bytes),
            Self::Absent { .. } => None,
        }
    }
}

pub(super) fn read_remote_file(
    env: &[(String, String)],
    path: &str,
    run: &mut impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> RemoteRead {
    let output = run(env, &["cat".to_owned(), path.to_owned()]);
    if output.success && !output.stdout.iter().all(u8::is_ascii_whitespace) {
        RemoteRead::Present(output.stdout)
    } else {
        RemoteRead::Absent {
            stderr: output.stderr,
        }
    }
}
