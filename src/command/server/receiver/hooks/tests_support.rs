use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::json;

use super::{
    command, install_for_home, lifecycle_installations, portable_root_command, replace_entry,
    update_json_file, update_json_file_with_temporary,
};

fn configured_command(path: &Path, event: &str) -> String {
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    settings["hooks"][event][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn run_configured(
    root: &Path,
    command: &str,
    env: &[(&str, &std::ffi::OsStr)],
    input: &serde_json::Value,
) -> std::process::Output {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(root)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}
