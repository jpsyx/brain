#[cfg(unix)]
#[test]
fn concurrent_successful_creates_all_survive_the_registry_transaction() {
    use std::collections::BTreeSet;
    const WRITERS: usize = 20;

    let fixture = Fixture::new();
    let initial = fixture.home.path().join("initial");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&initial)]));
    let before = fixture.registry();
    let initial_manifest = std::fs::read(WorkspaceManifest::path(&initial)).unwrap();
    let initial_config = std::fs::read(initial.join(".config/config.json")).unwrap();
    let release = fixture.current_dir.path().join("release-writers");
    let roots = (0..WRITERS)
        .map(|index| fixture.home.path().join(format!("concurrent-{index}")))
        .collect::<Vec<_>>();
    let mut waiters = Vec::with_capacity(WRITERS);
    for root in &roots {
        let child = fixture
            .barrier_command(&release, &["workspace", "create", "--root", path_arg(root)])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn blocked workspace writer");
        waiters.push(std::thread::spawn(move || child.wait_with_output()));
    }
    std::fs::write(&release, b"go").expect("release workspace writers");
    let outputs = waiters
        .into_iter()
        .map(|waiter| waiter.join().unwrap().expect("wait for workspace writer"))
        .collect::<Vec<_>>();
    let failures = outputs
        .iter()
        .enumerate()
        .filter(|(_, output)| !output.status.success())
        .map(|(index, output)| {
            format!(
                "writer {index} failed ({})\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(outputs.len(), WRITERS);
    let registry = fixture.registry();
    assert_eq!(registry.schema_version, before.schema_version);
    assert_eq!(registry.default_workspace, before.default_workspace);
    assert_eq!(registry.env, before.env);
    assert_eq!(registry.workspaces[&name("initial")], before.workspaces[&name("initial")]);
    assert_eq!(std::fs::read(WorkspaceManifest::path(&initial)).unwrap(), initial_manifest);
    assert_eq!(std::fs::read(initial.join(".config/config.json")).unwrap(), initial_config);
    let lock = fixture.registry_path().with_file_name(".env.json.transaction.lock");
    let mut expected_names = BTreeSet::from([name("initial")]);
    let mut successes = 0;
    for (index, output) in outputs.iter().enumerate() {
        let workspace_name = name(&format!("concurrent-{index}"));
        let root = &roots[index];
        if output.status.success() {
            successes += 1;
            expected_names.insert(workspace_name.clone());
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                format!("Registered workspace {workspace_name} at {}\n", root.display()),
                "{failures}"
            );
            let record = registry.workspaces.get(&workspace_name).expect("successful writer was lost");
            assert_eq!(&record.root, root);
            let manifest = WorkspaceManifest::load(root, env!("CARGO_PKG_VERSION")).unwrap();
            assert_eq!(manifest.workspace_id(), record.workspace_id);
        } else {
            assert!(registry_contention_refusal(output, &lock), "unexpected create result:\n{failures}");
            assert!(!registry.workspaces.contains_key(&workspace_name), "{failures}");
            assert_eq!(std::fs::symlink_metadata(root).unwrap_err().kind(), std::io::ErrorKind::NotFound);
        }
    }
    // This fixture needs multiple acknowledged writes to exercise its lost-update oracle.
    assert!(successes >= 2, "insufficient successful writers for this fixture:\n{failures}");
    assert_eq!(registry.workspaces.keys().cloned().collect::<BTreeSet<_>>(), expected_names);
}

#[cfg(unix)]
fn registry_contention_refusal(output: &Output, lock: &Path) -> bool {
    if output.status.code() != Some(1) || !output.stdout.is_empty() {
        return false;
    }
    let Ok(stderr) = std::str::from_utf8(&output.stderr) else { return false; };
    let Some(error) = stderr.trim().strip_prefix("Workspace error: Workspace error: ") else { return false; };
    let prefix = format!("workspace registry transaction lock at {} remained held", lock.display());
    let Some(owner) = error.strip_prefix(&prefix).and_then(|suffix| suffix.split_once(" after 10000ms;").map(|(owner, _)| owner)) else { return false; };
    let owner_pid = if owner.is_empty() {
        None
    } else {
        let Some(pid) = owner.strip_prefix(" by PID ").and_then(|pid| pid.parse::<u32>().ok()) else { return false; };
        Some(pid)
    };
    error == brain::workspace::RegistryError::LockTimeout { path: lock.to_path_buf(), owner_pid, waited_millis: 10_000 }.to_string()
}

#[cfg(unix)]
#[test]
fn concurrent_create_refusal_classifier_rejects_other_or_committed_failures() {
    use std::os::unix::process::ExitStatusExt as _;
    let lock = Path::new("/isolated/.env.json.transaction.lock");
    let error = brain::workspace::RegistryError::LockTimeout { path: lock.to_path_buf(), owner_pid: Some(1234), waited_millis: 10_000 };
    let stderr = format!("Workspace error: Workspace error: {error}\n").into_bytes();
    let mut output = Output { status: std::process::ExitStatus::from_raw(1 << 8), stdout: Vec::new(), stderr: stderr.clone() };
    assert!(registry_contention_refusal(&output, lock));
    assert!(!registry_contention_refusal(&output, Path::new("/different/lock")));
    output.status = std::process::ExitStatus::from_raw(2 << 8);
    assert!(!registry_contention_refusal(&output, lock));
    output.status = std::process::ExitStatus::from_raw(1 << 8);
    output.stdout = b"Registered workspace committed\n".to_vec();
    assert!(!registry_contention_refusal(&output, lock));
    output.stdout.clear();
    output.stderr = [b"workspace setup committed; mandatory startup did not complete: ".as_slice(), &stderr].concat();
    assert!(!registry_contention_refusal(&output, lock));
    output.stderr = b"Workspace error: unreadable_source [reconcile]\n".to_vec();
    assert!(!registry_contention_refusal(&output, lock));
}
