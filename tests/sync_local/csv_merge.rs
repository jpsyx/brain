use std::cell::Cell;

use brain::sync::csv_sync::{baseline_path, sync_one};

use super::*;

/// Drives the CSV three-way merge over a local fake remote. Local adds task A
/// and remote adds task B; both sides converge and a second pass is a no-op.
#[test]
fn csv_sync_one_converges_local_and_remote_and_is_idempotent() {
    let base = std::env::temp_dir().join(format!("brain-csv-it-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let local = base.join("local.csv");
    let remote = base.join("remote.csv");
    let paths = brain::workspace::WorkspacePaths::new(&base, brain::workspace::WorkspaceId::new());

    let rel = "tasks/tasks.csv";
    let name = Path::new(rel)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let baseline = baseline_path(&paths, &name);
    std::fs::remove_file(&baseline).ok();

    let header = "task_id,status,notes,last_touched\n";
    std::fs::write(&local, format!("{header}A,open,alpha,t1\n")).unwrap();
    std::fs::write(&remote, format!("{header}B,open,beta,t1\n")).unwrap();
    let pushes = Cell::new(0);

    let out = sync_one(
        &paths,
        &local,
        rel,
        || std::fs::read_to_string(&remote).ok(),
        |text| {
            pushes.set(pushes.get() + 1);
            std::fs::write(&remote, text).is_ok()
        },
    );
    assert_eq!(out.added, 2, "A and B are both new");
    assert_eq!(out.soft_conflicts, 0, "disjoint adds do not conflict");

    let merged = std::fs::read_to_string(&local).unwrap();
    assert_eq!(
        merged,
        std::fs::read_to_string(&remote).unwrap(),
        "local and remote converge"
    );
    assert!(
        merged.contains("A,open,alpha") && merged.contains("B,open,beta"),
        "merged holds the union of both sides: {merged}"
    );
    assert!(baseline.exists(), "baseline snapshot written");

    let second = sync_one(
        &paths,
        &local,
        rel,
        || std::fs::read_to_string(&remote).ok(),
        |text| {
            pushes.set(pushes.get() + 1);
            std::fs::write(&remote, text).is_ok()
        },
    );
    assert_eq!(second.added, 0, "nothing new on the second run");
    assert_eq!(merged, std::fs::read_to_string(&local).unwrap());
    assert_eq!(merged, std::fs::read_to_string(&remote).unwrap());
    assert_eq!(
        pushes.get(),
        1,
        "an unchanged pass must not rewrite the remote and re-arm the watcher"
    );

    std::fs::remove_dir_all(&base).ok();
    std::fs::remove_file(&baseline).ok();
}
