use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;
use std::time::{Duration, Instant};

use super::support::*;

#[test]
fn habits_cli_uses_the_selected_workspaces_accepted_ingress_after_manifest_mismatch() {
    let server = ServerFixture::new(FAMILY_ID);
    std::fs::write(
        server.family_root.join(".config/workspace.json"),
        format!(
            "{{\"schema_version\":1,\"workspace_id\":\"{FAMILY_ID}\",\"receiver_ingress_id\":\"{}\",\"minimum_brain_version\":\"0.28.0\"}}\n",
            server.personal_ingress
        ),
    )
    .expect("replace only the family manifest ingress");
    let fake_bin = server.home.path().join("bin");
    std::fs::create_dir(&fake_bin).expect("create fake binary directory");
    let fake_open = fake_bin.join("open");
    std::fs::write(
        &fake_open,
        "#!/bin/sh\nprintf '%s\\n' \"$1\" > \"$BRAIN_TEST_OPENED_URL\"\n",
    )
    .expect("write fake open command");
    std::fs::set_permissions(&fake_open, std::fs::Permissions::from_mode(0o755))
        .expect("make fake open executable");
    let opened_url = server.home.path().join("opened-url");
    let path = std::env::var_os("PATH").unwrap_or_default();

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["habits", "-b", "family"])
        .env("HOME", server.home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), path.to_string_lossy()),
        )
        .env("BRAIN_TEST_OPENED_URL", &opened_url)
        .output()
        .expect("run habits command");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    wait_until(Instant::now() + Duration::from_secs(1), || {
        opened_url.is_file()
    });
    let opened = std::fs::read_to_string(&opened_url).expect("captured habits URL");

    assert!(
        opened.contains(&server.family_ingress.to_string()),
        "{opened}"
    );
    assert!(
        !opened.contains(&server.personal_ingress.to_string()),
        "{opened}"
    );
}
