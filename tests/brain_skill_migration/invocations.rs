use super::*;
use fs2::FileExt as _;

fn lock(f: &Fixture) -> std::fs::File {
    let path = f.home.join(".cache/brain/invocations.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap()
}

#[test]
fn ordinary_and_hidden_maintenance_refuse_an_active_installation() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    let held = lock(&f);
    held.lock_exclusive().unwrap();
    for args in [
        vec!["server", "status"],
        vec![
            "__migrate",
            "--from-version",
            env!("CARGO_PKG_VERSION"),
            "--to-version",
            env!("CARGO_PKG_VERSION"),
        ],
    ] {
        let output = f.run(&args);
        assert!(
            !output.status.success(),
            "maintenance bypassed active installation"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("retry"));
    }
    assert!(!f.root.join("AGENTS.md").exists());
}

#[test]
fn hidden_refresh_refuses_active_modern_invocation_and_live_historical_tui() {
    for modern in [true, false] {
        let f = Fixture::new(env!("CARGO_PKG_VERSION"));
        installer::compatible(&f);
        let held = modern.then(|| {
            let held = lock(&f);
            fs2::FileExt::lock_shared(&held).unwrap();
            held
        });
        if !modern {
            std::fs::create_dir_all(f.cache()).unwrap();
            std::fs::write(
                f.cache().join("tui.lock"),
                format!("{}\n", std::process::id()),
            )
            .unwrap();
        }
        let output = installer::refresh(
            &f,
            &installer::target(&f, "0.101.0"),
            "0.101.0",
            "preflight",
        );
        assert!(
            !output.status.success(),
            "live process did not prevent installation; modern={modern}"
        );
        assert!(!f.root.join(".agents").exists());
        drop(held);
    }
}

#[test]
fn help_and_version_do_not_create_invocation_state_or_wait_for_an_installer() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    for args in [["--help"], ["--version"]] {
        assert!(f.run(&args).status.success());
    }
    assert!(!f.home.join(".cache").exists());
    let held = lock(&f);
    held.lock_exclusive().unwrap();
    for args in [["--help"], ["--version"]] {
        assert!(f.run(&args).status.success());
    }
}

#[test]
fn ordinary_command_already_waiting_for_sync_prevents_installer_replacement() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    installer::compatible(&f);
    f.startup();
    let extensions = f.root.join(".config/extensions");
    std::fs::create_dir_all(&extensions).unwrap();
    std::fs::write(extensions.join("agenda.md"), "[agenda:anchors]\nWAITING\n").unwrap();
    let sync = brain::sync::lock::try_acquire(&f.cache().join("sync/sync.lock")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["server", "status"])
        .env_clear()
        .env("HOME", &f.home)
        .env("XDG_CONFIG_HOME", f.home.join(".config"))
        .env("PATH", "/usr/bin:/bin")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let observed = lock(&f);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match observed.try_lock_exclusive() {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Ok(()) => fs2::FileExt::unlock(&observed).unwrap(),
            Err(error) => panic!("{error}"),
        }
        assert!(
            std::time::Instant::now() < deadline,
            "ordinary child never held invocation ownership"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = installer::refresh(&f, &installer::target(&f, "0.101.0"), "0.101.0", "apply");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Brain is active"));
    drop(sync);
    assert!(child.wait_with_output().unwrap().status.success());
    assert_eq!(
        read_json(&f.cache().join("skills-install/managed.json"))["schema_version"],
        2
    );
}
