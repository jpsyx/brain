use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixListener;

const FAMILY_ID: &str = "11111111-1111-4111-8111-111111111111";
const WORK_ID: &str = "22222222-2222-4222-8222-222222222222";

fn legacy_socket(fixture: &Fixture, workspace_id: &str) -> PathBuf {
    fixture
        .home
        .join(".cache/brain/workspaces")
        .join(workspace_id)
        .join("jobs.sock")
}

fn bind_stale_socket(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("legacy socket parent"))
        .expect("legacy socket directory");
    drop(UnixListener::bind(path).expect("stale legacy socket"));
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("legacy owner-only permissions");
}

#[test]
fn ordinary_startup_removes_exact_stale_legacy_sockets_for_every_workspace() {
    let fixture = Fixture::new();
    let sockets = [
        legacy_socket(&fixture, FAMILY_ID),
        legacy_socket(&fixture, WORK_ID),
    ];
    for socket in &sockets {
        bind_stale_socket(socket);
    }

    let output = fixture.run(&["server", "status"]);

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    for socket in sockets {
        assert!(!socket.exists(), "stale legacy socket survived cutover");
    }
}

#[test]
fn ordinary_startup_preserves_live_socket_symlink_and_regular_file_leaves() {
    for kind in ["live", "symlink", "regular", "permissive"] {
        let fixture = Fixture::new();
        let socket = legacy_socket(&fixture, FAMILY_ID);
        std::fs::create_dir_all(socket.parent().expect("legacy socket parent"))
            .expect("legacy socket directory");
        let _listener = match kind {
            "live" => {
                let listener = UnixListener::bind(&socket).expect("live legacy socket");
                std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
                    .expect("legacy owner-only permissions");
                Some(listener)
            }
            "symlink" => {
                let target = fixture.home.join("replacement");
                std::fs::write(&target, "keep").expect("symlink target");
                symlink(&target, &socket).expect("legacy socket symlink");
                None
            }
            "regular" => {
                std::fs::write(&socket, "replacement").expect("regular replacement");
                None
            }
            "permissive" => {
                let listener = UnixListener::bind(&socket).expect("permissive legacy socket");
                drop(listener);
                std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666))
                    .expect("permissive replacement permissions");
                None
            }
            _ => unreachable!(),
        };

        let output = fixture.run(&["server", "status"]);

        assert!(output.status.success(), "{kind}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(std::fs::symlink_metadata(&socket).is_ok(), "{kind} leaf was removed");
    }
}

#[test]
fn ordinary_startup_repairs_a_stale_socket_created_after_the_version_stamp() {
    let fixture = Fixture::new();
    let first = fixture.run(&["server", "status"]);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let socket = legacy_socket(&fixture, FAMILY_ID);
    bind_stale_socket(&socket);

    let second = fixture.run(&["server", "status"]);

    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    assert!(!socket.exists(), "same-version startup did not repair the stale socket");
}

#[test]
fn help_and_version_leave_an_exact_stale_legacy_socket_untouched() {
    for arguments in [["--help"].as_slice(), ["--version"].as_slice()] {
        let fixture = Fixture::new();
        let socket = legacy_socket(&fixture, FAMILY_ID);
        bind_stale_socket(&socket);

        let output = fixture.run(arguments);

        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(socket.exists(), "{arguments:?} ran the cutover migration");
    }
}

#[test]
fn explicit_downgrade_is_idempotent_and_does_not_create_a_legacy_socket() {
    let fixture = Fixture::new();
    let socket = legacy_socket(&fixture, FAMILY_ID);

    for _ in 0..2 {
        let output = fixture.run(&[
            "__migrate",
            "--from-version",
            env!("CARGO_PKG_VERSION"),
            "--to-version",
            "0.86.1",
        ]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(std::fs::symlink_metadata(&socket).is_err());
    }
}
