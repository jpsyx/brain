use super::external::{fixture, progress, rows, start};
use crate::tasks::removal::resume;

fn check_failures(stage: &str) {
    for (cause, failure) in [
        ("timeout", "import time; time.sleep(60)"),
        ("protocol", "print('invalid-json-diagnostic'); sys.exit(0)"),
        ("provider_error", "status='error'"),
    ] {
        let setup = if stage == "status" {
            "if r['action']=='cancel': status='error'\n"
        } else {
            ""
        };
        let f = fixture(&format!("{setup}if r['action']=='{stage}': {failure}"));
        let before = rows(&f);
        let initial = start(&f).unwrap_err();
        let id = initial.details["operation_id"].as_str().unwrap();
        let error = if stage == "status" {
            resume(&f.command(), id).unwrap_err()
        } else {
            initial.clone()
        };
        assert_eq!(error.code, "external_pending");
        assert_eq!(error.details["stage"], stage, "{cause}");
        assert_eq!(error.details["cause"], cause, "{stage}");
        assert_eq!(error.details["operation_id"], id);
        assert!(error.details["action_id"].is_string());
        assert_eq!(rows(&f), before);
        let saved = progress(&f, id);
        let expected = if stage == "preflight" {
            "pending"
        } else {
            "unknown"
        };
        assert_eq!(saved["external"][0]["state"], expected);
        for value in [serde_json::to_string(&error).unwrap(), saved.to_string()] {
            assert!(!value.contains("provider diagnostic"));
            assert!(!value.contains("invalid-json-diagnostic"));
        }
    }
}

#[test]
fn removal_external_failure_causes_survive_cancel_without_resolving_unknown_intent() {
    check_failures("cancel");
}

#[test]
fn removal_external_failure_causes_survive_status_without_resolving_unknown_intent() {
    check_failures("status");
}

#[test]
fn removal_external_failure_causes_survive_preflight_without_cancellation() {
    check_failures("preflight");
}
