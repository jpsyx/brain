use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Directory {
        mode: u32,
        children: BTreeMap<OsString, Self>,
    },
    File {
        mode: u32,
        bytes: Vec<u8>,
        sha256: [u8; 32],
    },
    Symlink {
        mode: u32,
        target: PathBuf,
        referent: Box<Referent>,
    },
    Other {
        file_type: OtherFileType,
        metadata: UnixIdentity,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum OtherFileType {
    BlockDevice,
    CharacterDevice,
    Fifo,
    Socket,
    Other,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct UnixIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    hard_links: u64,
    uid: u32,
    gid: u32,
    special_device: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Referent {
    Entry(Entry),
    Cycle { device: u64, inode: u64 },
    Missing,
}

pub(super) fn snapshot(root: &Path) -> Entry {
    let metadata = std::fs::metadata(root).expect("snapshot root metadata");
    let mut ancestors = BTreeSet::from([(metadata.dev(), metadata.ino())]);
    snapshot_referent(root, &metadata, &mut ancestors)
}

pub(super) fn snapshot_entry(path: &Path) -> Entry {
    let metadata = std::fs::symlink_metadata(path).expect("snapshot entry metadata");
    let mut ancestors = BTreeSet::new();
    snapshot_child_referent(path, &metadata, &mut ancestors)
}

fn snapshot_referent(
    path: &Path,
    metadata: &std::fs::Metadata,
    ancestors: &mut BTreeSet<(u64, u64)>,
) -> Entry {
    if metadata.is_dir() {
        let mut entries = BTreeMap::new();
        snapshot_directory(path, &mut entries, ancestors);
        Entry::Directory {
            mode: metadata.mode(),
            children: entries,
        }
    } else if metadata.is_file() {
        let bytes = std::fs::read(path).expect("snapshot file");
        let sha256 = Sha256::digest(&bytes).into();
        Entry::File {
            mode: metadata.mode(),
            bytes,
            sha256,
        }
    } else {
        Entry::Other {
            file_type: other_file_type(metadata.file_type()),
            metadata: unix_identity(metadata),
        }
    }
}

fn other_file_type(file_type: std::fs::FileType) -> OtherFileType {
    if file_type.is_socket() {
        OtherFileType::Socket
    } else if file_type.is_fifo() {
        OtherFileType::Fifo
    } else if file_type.is_block_device() {
        OtherFileType::BlockDevice
    } else if file_type.is_char_device() {
        OtherFileType::CharacterDevice
    } else {
        OtherFileType::Other
    }
}

fn unix_identity(metadata: &std::fs::Metadata) -> UnixIdentity {
    UnixIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        hard_links: metadata.nlink(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        special_device: metadata.rdev(),
        size: metadata.size(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    }
}

fn snapshot_directory(
    directory: &Path,
    entries: &mut BTreeMap<OsString, Entry>,
    ancestors: &mut BTreeSet<(u64, u64)>,
) {
    let mut children = std::fs::read_dir(directory)
        .expect("read snapshot directory")
        .map(|entry| entry.expect("read snapshot entry"))
        .collect::<Vec<_>>();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let path = child.path();
        entries.insert(child.file_name(), snapshot_path(&path, ancestors));
    }
}

fn snapshot_path(path: &Path, ancestors: &mut BTreeSet<(u64, u64)>) -> Entry {
    let link_metadata = std::fs::symlink_metadata(path).expect("snapshot path metadata");
    if link_metadata.file_type().is_symlink() {
        let referent = match std::fs::metadata(path) {
            Ok(metadata) => snapshot_link_referent(path, &metadata, ancestors),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Referent::Missing,
            Err(error) => panic!("snapshot symlink referent: {error}"),
        };
        return Entry::Symlink {
            mode: link_metadata.mode(),
            target: std::fs::read_link(path).expect("snapshot symlink target"),
            referent: Box::new(referent),
        };
    }
    snapshot_child_referent(path, &link_metadata, ancestors)
}

fn snapshot_link_referent(
    path: &Path,
    metadata: &std::fs::Metadata,
    ancestors: &mut BTreeSet<(u64, u64)>,
) -> Referent {
    let identity = (metadata.dev(), metadata.ino());
    if metadata.is_dir() && ancestors.contains(&identity) {
        return Referent::Cycle {
            device: identity.0,
            inode: identity.1,
        };
    }
    Referent::Entry(snapshot_child_referent(path, metadata, ancestors))
}

fn snapshot_child_referent(
    path: &Path,
    metadata: &std::fs::Metadata,
    ancestors: &mut BTreeSet<(u64, u64)>,
) -> Entry {
    let identity = (metadata.dev(), metadata.ino());
    let inserted = metadata.is_dir() && ancestors.insert(identity);
    let entry = snapshot_referent(path, metadata, ancestors);
    if inserted {
        ancestors.remove(&identity);
    }
    entry
}

#[test]
fn detects_a_mode_only_mutation() {
    let home = tempfile::tempdir().expect("temporary home");
    let file = home.path().join("config.json");
    std::fs::write(&file, b"same bytes").expect("fixture file");
    let before = snapshot(home.path());

    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
        .expect("change fixture mode");

    assert_ne!(snapshot(home.path()), before);
}

#[test]
fn detects_a_symlink_referent_mutation_and_terminates_cycles() {
    let home = tempfile::tempdir().expect("temporary home");
    let external = tempfile::tempdir().expect("external referent");
    let referent = external.path().join("config.json");
    std::fs::write(&referent, b"before").expect("referent file");
    std::os::unix::fs::symlink(external.path(), home.path().join("linked"))
        .expect("fixture symlink");
    std::os::unix::fs::symlink(home.path(), external.path().join("cycle")).expect("cycle symlink");
    let before = snapshot(home.path());

    std::fs::write(&referent, b"after").expect("mutate referent");

    assert_ne!(snapshot(home.path()), before);
}

#[test]
fn detects_same_mode_unix_socket_replacement() {
    let home = tempfile::tempdir().expect("temporary home");
    let socket = home.path().join("control.sock");
    let first = std::os::unix::net::UnixListener::bind(&socket).expect("first socket");
    let before = snapshot(home.path());

    std::fs::remove_file(&socket).expect("remove first socket name");
    let second = std::os::unix::net::UnixListener::bind(&socket).expect("replacement socket");

    assert_ne!(snapshot(home.path()), before);
    drop((first, second));
}
