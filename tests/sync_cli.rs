use std::process::Command;

fn brain(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(args)
        .output()
        .expect("run brain binary");
    assert!(
        output.status.success(),
        "brain {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("sync help output is utf-8")
}

#[test]
fn sync_help_advertises_repair_not_init() {
    let help = brain(&["sync", "--help"]);

    assert!(help.contains("repair"), "{help}");
    assert!(!help.contains("init"), "{help}");
}
