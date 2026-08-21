
#[derive(Debug, Clone, PartialEq, Eq)]
struct RunLogEntry {
    device: u64,
    inode: u64,
    mode: u32,
    hard_links: u64,
    uid: u32,
    gid: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
    bytes: Vec<u8>,
    sha256: [u8; 32],
}

fn run_log_snapshot() -> BTreeMap<PathBuf, RunLogEntry> {
    std::fs::read_dir("/tmp")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy();
            is_brain_run_log_name(&name).then(|| {
                let metadata = std::fs::metadata(&path).expect("run log metadata");
                let bytes = std::fs::read(&path).expect("run log bytes");
                let sha256 = Sha256::digest(&bytes).into();
                (
                    path,
                    RunLogEntry {
                        device: metadata.dev(),
                        inode: metadata.ino(),
                        mode: metadata.mode(),
                        hard_links: metadata.nlink(),
                        uid: metadata.uid(),
                        gid: metadata.gid(),
                        size: metadata.len(),
                        modified_seconds: metadata.mtime(),
                        modified_nanoseconds: metadata.mtime_nsec(),
                        changed_seconds: metadata.ctime(),
                        changed_nanoseconds: metadata.ctime_nsec(),
                        bytes,
                        sha256,
                    },
                )
            })
        })
        .collect()
}

fn is_brain_run_log_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".log") else {
        return false;
    };
    let Some((timestamp, pid)) = stem.rsplit_once('-') else {
        return false;
    };
    timestamp.contains('T') && pid.chars().all(|character| character.is_ascii_digit())
}

fn pid_run_logs(
    pid: u32,
    snapshot: &BTreeMap<PathBuf, RunLogEntry>,
) -> BTreeMap<PathBuf, RunLogEntry> {
    let suffix = format!("-{pid}.log");
    snapshot
        .iter()
        .filter(|(path, _)| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
        })
        .map(|(path, entry)| (path.clone(), entry.clone()))
        .collect()
}
