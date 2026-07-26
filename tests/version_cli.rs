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
    String::from_utf8(output.stdout).expect("version output is utf-8")
}

#[test]
fn version_surfaces_match_the_crate_version() {
    let expected = "brain 0.2.0\n";
    assert_eq!(brain(&["--version"]), expected);
    assert_eq!(brain(&["-v"]), expected);
    assert_eq!(brain(&["version"]), expected);
}
