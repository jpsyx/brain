use std::os::unix::fs::PermissionsExt as _;

use super::{compatibility, lists_a_model, parse_version};

const FULL_HELP: &str = "Usage:\n  pi [options] [--] [@files...] [messages...]\n\nOptions:\n  --append-system-prompt <text>  Append text to the system prompt\n  --session-id <id>              Use exact project session ID, creating it if missing\n  --extension, -e <path>         Load an extension file\n  --skill <path>                 Load a skill file or directory\n  --no-skills, -ns               Disable skills discovery\n  --no-approve, -na              Ignore project-local files for this run\n";

const HEALTHY_MODELS: &str =
    "provider      model          context  max-out  thinking  images\nexample       example-model  1M       16.4K    yes       no\n";

struct FakePi {
    _directory: tempfile::TempDir,
    command: String,
}

/// A stand-in `pi` answering the three read-only questions the probe asks.
fn fake_pi(version: &str, help: &str, models: &str, catalog_status: u8) -> FakePi {
    let directory = tempfile::tempdir().expect("temporary pi command");
    for (name, contents) in [
        ("version.txt", version),
        ("help.txt", help),
        ("models.txt", models),
    ] {
        std::fs::write(directory.path().join(name), contents).expect("write fake pi output");
    }
    let script = directory.path().join("fake-pi");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nhere=$(dirname \"$0\")\ncase \"$1\" in\n  --version) cat \"$here/version.txt\" ;;\n  --help) cat \"$here/help.txt\" ;;\n  --list-models) cat \"$here/models.txt\"; exit {catalog_status} ;;\n  *) exit 64 ;;\nesac\n"
        ),
    )
    .expect("write fake pi command");
    let mut permissions = std::fs::metadata(&script)
        .expect("fake pi metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).expect("make fake pi executable");
    let command = crate::agent::frontend::shell_quote(&script.display().to_string());
    FakePi {
        _directory: directory,
        command,
    }
}

fn failure(fake: &FakePi) -> String {
    compatibility(&fake.command)
        .expect_err("incompatible pi")
        .to_string()
}

#[test]
fn a_current_pi_with_a_usable_model_is_compatible() {
    let fake = fake_pi("0.85.1\n", FULL_HELP, HEALTHY_MODELS, 0);

    assert_eq!(
        compatibility(&fake.command),
        Ok(Some("0.85.1".to_owned())),
        "a healthy pi reports its version"
    );
}

#[test]
fn an_unavailable_command_names_the_env_variable_that_fixes_it() {
    assert_eq!(
        compatibility("brain-missing-pi-command-7f31c0aa")
            .expect_err("unavailable pi")
            .to_string(),
        format!("frontend error: {}", super::UNAVAILABLE)
    );
}

#[test]
fn a_version_below_the_floor_is_rejected_and_names_the_floor() {
    let fake = fake_pi("0.84.0\n", FULL_HELP, HEALTHY_MODELS, 0);

    let message = failure(&fake);

    assert!(message.contains("version 0.84.0 is older than the 0.84.1"), "{message}");
    assert!(message.contains("brain env set pi_cmd"), "{message}");
}

#[test]
fn the_exact_floor_and_a_prerelease_of_a_newer_version_are_accepted() {
    assert_eq!(
        compatibility(&fake_pi("0.84.1\n", FULL_HELP, HEALTHY_MODELS, 0).command),
        Ok(Some("0.84.1".to_owned()))
    );
    assert_eq!(
        compatibility(&fake_pi("0.86.0-rc.1\n", FULL_HELP, HEALTHY_MODELS, 0).command),
        Ok(Some("0.86.0-rc.1".to_owned()))
    );
}

#[test]
fn output_from_some_other_program_is_not_a_pi_version() {
    for output in ["Python 3.9.6\n", "pi 0.85.1\n", "current\n"] {
        let fake = fake_pi(output, FULL_HELP, HEALTHY_MODELS, 0);

        assert_eq!(
            failure(&fake),
            format!("frontend error: {}", super::MALFORMED),
            "{output:?}"
        );
    }
}

#[test]
fn a_build_missing_a_flag_brain_appends_names_that_flag() {
    let help = FULL_HELP.replace("  --no-approve, -na              Ignore project-local files for this run\n", "");
    let fake = fake_pi("0.85.1\n", &help, HEALTHY_MODELS, 0);

    let message = failure(&fake);

    assert!(message.contains("does not advertise `--no-approve`"), "{message}");
}

#[test]
fn a_pi_with_no_resolvable_provider_credentials_is_reported_as_unconfigured() {
    let fake = fake_pi(
        "0.85.1\n",
        FULL_HELP,
        "No models available. Use /login to log into a provider via OAuth or API key.\n",
        0,
    );

    assert_eq!(failure(&fake), format!("frontend error: {}", super::NO_MODELS));
}

#[test]
fn a_catalog_that_cannot_be_read_is_reported_separately_from_an_empty_one() {
    let fake = fake_pi("0.85.1\n", FULL_HELP, "", 1);

    assert_eq!(
        failure(&fake),
        format!("frontend error: {}", super::UNREADABLE_CATALOG)
    );
}

#[test]
fn a_model_table_needs_a_header_and_a_row() {
    assert!(lists_a_model(HEALTHY_MODELS));
    assert!(!lists_a_model(
        "provider      model          context  max-out  thinking  images\n"
    ));
    assert!(!lists_a_model("No models available.\n"));
    assert!(!lists_a_model(""));
}

#[test]
fn version_parsing_accepts_the_shapes_pi_prints_and_nothing_else() {
    assert_eq!(
        parse_version("0.85.1\n"),
        Some(("0.85.1".to_owned(), (0, 85, 1)))
    );
    assert_eq!(
        parse_version("v1.0.0"),
        Some(("1.0.0".to_owned(), (1, 0, 0)))
    );
    for rejected in ["0.85", "0.85.1.2", "pi 0.85.1", "", "x.y.z"] {
        assert_eq!(parse_version(rejected), None, "{rejected:?}");
    }
}
