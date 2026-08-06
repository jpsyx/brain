use std::process::Command;

use super::RemoteCommandOutput;

pub(super) fn run_remote_command(env: &[(String, String)], args: &[String]) -> RemoteCommandOutput {
    crate::logging::log(format!(
        "spawn rclone identity args={args:?} env_keys={}",
        env.len()
    ));
    let mut command = Command::new("rclone");
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    match command.output() {
        Ok(output) => RemoteCommandOutput {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => RemoteCommandOutput {
            success: false,
            stdout: Vec::new(),
            stderr: error.to_string(),
        },
    }
}
