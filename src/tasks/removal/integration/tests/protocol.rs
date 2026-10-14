use super::request;
use crate::tasks::removal::integration::protocol::{ErrorKind, decode};
use serde_json::json;

#[test]
fn removal_integration_protocol_matches_identity_version_and_exact_action_status_pairs() {
    for action in ["preflight", "cancel", "status", "bogus"] {
        for status in [
            "ready",
            "canceled",
            "already_canceled",
            "pending",
            "error",
            "bogus",
        ] {
            let mut request = request();
            request.action = action.into();
            let response = json!({"schema_version":1,"operation_id":"operation-1","action_id":"action-1","status":status,"message":null});
            let allowed = matches!(
                (action, status),
                ("preflight", "ready" | "error")
                    | ("cancel", "canceled" | "already_canceled" | "error")
                    | (
                        "status",
                        "canceled" | "already_canceled" | "pending" | "error"
                    )
            );
            assert_eq!(
                decode(&request, &serde_json::to_vec(&response).unwrap()).is_ok(),
                allowed,
                "{action}/{status}"
            );
        }
    }
    for (key, value) in [
        ("schema_version", json!(2)),
        ("operation_id", json!("other")),
        ("action_id", json!("other")),
        ("extra", json!(true)),
    ] {
        let mut response = json!({"schema_version":1,"operation_id":"operation-1","action_id":"action-1","status":"canceled"});
        response[key] = value;
        assert_eq!(
            decode(&request(), &serde_json::to_vec(&response).unwrap())
                .unwrap_err()
                .kind,
            ErrorKind::Protocol
        );
    }
    for response in [b"{}".as_slice(), b"not JSON", b"\xff", br#"{"schema_version":1,"operation_id":"operation-1","action_id":"action-1","status":"canceled"} {}"#] {
        assert_eq!(decode(&request(), response).unwrap_err().kind, ErrorKind::Protocol);
    }
}
