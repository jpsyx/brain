//! Hoisting machine-global env out of the workspace records.
//!
//! Schema v2 gave every workspace record its own `env` map, which was right for
//! roots, launch commands, and provider credentials but wrong for values that
//! describe the machine: `markdown_to_pdf_path` is the location of one binary,
//! and `brain_receiver_public_url` is the one public origin whose `/sms` and
//! `/email` URLs every workspace's provider portal is pointed at. A machine has
//! exactly one answer for each no matter how many workspaces it has registered.
//! v3 added the top-level `env` object for machine-scoped values and hoisted the
//! first; v4 hoists the receiver origin the same way, once brain began routing
//! by the number or address a message arrived at rather than by the URL.
//!
//! Pure: it rewrites JSON only. The transaction, backup, and save live in
//! [`super::migrate`].

use serde_json::{Map, Value};

/// Every schema this upgrade can read, all of which differ from the current one
/// only in which env values had been hoisted yet.
const UPGRADABLE_SCHEMA_VERSIONS: [u64; 2] = [2, 3];

/// Keys that move from every workspace record into the machine-global map.
///
/// The env schema is the single source of truth for which variables are
/// machine-scoped, so adding one there is all a future hoist needs.
use crate::env::MACHINE_GLOBAL_VARS as HOISTED_KEYS;

/// Rewrite an older registry at the current schema, or `None` when `value` is
/// not an upgradable registry (already current, a legacy flat env, or not an
/// object at all).
///
/// A machine that somehow holds several values for a hoisted key keeps the
/// first in canonical workspace-name order and drops the rest: they describe one
/// machine, so any of them is as good as another, and picking deterministically
/// means every machine and every retry agrees.
#[must_use]
pub(super) fn upgrade_to_current(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let found = object.get("schema_version").and_then(Value::as_u64)?;
    if !UPGRADABLE_SCHEMA_VERSIONS.contains(&found) {
        return None;
    }
    let mut upgraded = object.clone();
    let mut global = upgraded
        .get("env")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in HOISTED_KEYS {
        if let Some(hoisted) = take_from_workspaces(&mut upgraded, key) {
            global.entry(key.to_owned()).or_insert(hoisted);
        }
    }
    if global.is_empty() {
        upgraded.remove("env");
    } else {
        upgraded.insert("env".to_owned(), Value::Object(global));
    }
    upgraded.insert(
        "schema_version".to_owned(),
        Value::from(super::REGISTRY_SCHEMA_VERSION),
    );
    Some(Value::Object(upgraded))
}

/// Remove `key` from every workspace record's env, returning the first value
/// found in canonical-name order.
fn take_from_workspaces(registry: &mut Map<String, Value>, key: &str) -> Option<Value> {
    let workspaces = registry.get_mut("workspaces")?.as_object_mut()?;
    let mut hoisted = None;
    // `serde_json::Map` is a `BTreeMap`, so this walks canonical names in sorted
    // order: "first found" is the same on every machine and every retry.
    for record in workspaces.values_mut() {
        let Some(env) = record.as_object_mut().and_then(|r| r.get_mut("env")) else {
            continue;
        };
        let Some(env) = env.as_object_mut() else {
            continue;
        };
        let Some(found) = env.remove(key) else {
            continue;
        };
        if hoisted.is_none() && !is_blank(&found) {
            hoisted = Some(found);
        }
    }
    hoisted
}

/// Whether a stored value carries nothing worth hoisting.
fn is_blank(value: &Value) -> bool {
    value.as_str().is_some_and(|text| text.trim().is_empty()) || value.is_null()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v2(workspaces: &Value) -> Value {
        json!({
            "schema_version": 2,
            "default_workspace": "brain",
            "workspaces": workspaces,
        })
    }

    fn record(env: &Value) -> Value {
        json!({
            "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            "root": "/home/tester/brain",
            "aliases": [],
            "local_user_id": "pablo",
            "receiver_enabled": false,
            "env": env,
        })
    }

    #[test]
    fn the_only_configured_path_becomes_the_machine_global_one() {
        let upgraded = upgrade_to_current(&v2(&json!({
            "brain": record(&json!({"markdown_to_pdf_path": "/opt/markdown-to-pdf"})),
        })))
        .expect("older registry upgrades");

        assert_eq!(upgraded["schema_version"], json!(4));
        assert_eq!(
            upgraded["env"]["markdown_to_pdf_path"],
            "/opt/markdown-to-pdf"
        );
        // It must not remain where a workspace-scoped read would still find it.
        assert!(
            upgraded["workspaces"]["brain"]["env"]
                .get("markdown_to_pdf_path")
                .is_none()
        );
    }

    #[test]
    fn several_configured_paths_collapse_to_the_first_canonical_workspace() {
        let upgraded = upgrade_to_current(&v2(&json!({
            "personal": record(&json!({"markdown_to_pdf_path": "/personal/bin"})),
            "family": record(&json!({"markdown_to_pdf_path": "/family/bin"})),
            "brain": record(&json!({"markdown_to_pdf_path": "/brain/bin"})),
        })))
        .expect("older registry upgrades");

        // Canonical-name order, so the answer never depends on file layout.
        assert_eq!(upgraded["env"]["markdown_to_pdf_path"], "/brain/bin");
        for workspace in ["brain", "family", "personal"] {
            assert!(
                upgraded["workspaces"][workspace]["env"]
                    .get("markdown_to_pdf_path")
                    .is_none(),
                "{workspace} kept a workspace-scoped copy"
            );
        }
    }

    #[test]
    fn a_blank_value_never_wins_over_a_real_one() {
        // An empty string resolves to "unset"; hoisting it would lose the only
        // real path on the machine.
        let upgraded = upgrade_to_current(&v2(&json!({
            "brain": record(&json!({"markdown_to_pdf_path": "   "})),
            "family": record(&json!({"markdown_to_pdf_path": "/family/bin"})),
        })))
        .expect("older registry upgrades");

        assert_eq!(upgraded["env"]["markdown_to_pdf_path"], "/family/bin");
    }

    #[test]
    fn a_machine_that_never_configured_one_gains_no_global_env() {
        let upgraded = upgrade_to_current(&v2(&json!({
            "brain": record(&json!({"claude_cmd": "claude"})),
        })))
        .expect("older registry upgrades");

        assert_eq!(upgraded["schema_version"], json!(4));
        assert!(upgraded.get("env").is_none());
    }

    #[test]
    fn every_other_field_and_env_key_survives_untouched() {
        let original = v2(&json!({
            "brain": record(&json!({
                "markdown_to_pdf_path": "/opt/markdown-to-pdf",
                "claude_cmd": "claude --dangerously-skip-permissions",
                "sync": {"enabled": true, "b2_bucket": "keep"},
            })),
        }));

        let upgraded = upgrade_to_current(&original).expect("older registry upgrades");

        assert_eq!(upgraded["default_workspace"], "brain");
        let record = &upgraded["workspaces"]["brain"];
        assert_eq!(record["local_user_id"], "pablo");
        assert_eq!(
            record["workspace_id"],
            original["workspaces"]["brain"]["workspace_id"]
        );
        assert_eq!(
            record["env"]["claude_cmd"],
            "claude --dangerously-skip-permissions"
        );
        assert_eq!(record["env"]["sync"]["b2_bucket"], "keep");
    }

    #[test]
    fn an_existing_global_value_is_never_overwritten_by_a_record() {
        // A partially-upgraded file (global written, record not yet cleaned)
        // must converge on the global answer.
        let mut registry = v2(&json!({
            "brain": record(&json!({"markdown_to_pdf_path": "/stale/bin"})),
        }));
        registry["env"] = json!({"markdown_to_pdf_path": "/global/bin"});

        let upgraded = upgrade_to_current(&registry).expect("older registry upgrades");

        assert_eq!(upgraded["env"]["markdown_to_pdf_path"], "/global/bin");
        assert!(
            upgraded["workspaces"]["brain"]["env"]
                .get("markdown_to_pdf_path")
                .is_none()
        );
    }

    #[test]
    fn anything_that_is_not_an_older_registry_is_left_alone() {
        // Already current: the caller parses it directly.
        let mut current = v2(&json!({"brain": record(&json!({}))}));
        current["schema_version"] = json!(super::super::REGISTRY_SCHEMA_VERSION);
        assert!(upgrade_to_current(&current).is_none());
        // A legacy flat env has no schema version at all.
        assert!(upgrade_to_current(&json!({"root": "~/brain"})).is_none());
        assert!(upgrade_to_current(&json!("not an object")).is_none());
    }

    /// A schema-v3 registry: the machine-global map exists, but the receiver
    /// origin is still filed under each workspace.
    fn v3(workspaces: &Value) -> Value {
        let mut registry = v2(workspaces);
        registry["schema_version"] = json!(3);
        registry
    }

    #[test]
    fn v3_hoists_the_one_public_receiver_origin_out_of_the_records() {
        // There is one URL per channel for the machine, so a per-workspace copy
        // would be a second answer to a question with only one.
        let upgraded = upgrade_to_current(&v3(&json!({
            "brain": record(&json!({
                "brain_receiver_public_url": "https://brain.example.test",
                "twilio_auth_token": "keep-me",
            })),
            "family": record(&json!({
                "brain_receiver_public_url": "https://brain.example.test",
            })),
        })))
        .expect("older registry upgrades");

        assert_eq!(upgraded["schema_version"], json!(4));
        assert_eq!(
            upgraded["env"]["brain_receiver_public_url"],
            "https://brain.example.test"
        );
        for workspace in ["brain", "family"] {
            assert!(
                upgraded["workspaces"][workspace]["env"]
                    .get("brain_receiver_public_url")
                    .is_none(),
                "{workspace} kept a workspace-scoped origin"
            );
        }
        // Credentials stay where they belong: one workspace, one Twilio token.
        assert_eq!(
            upgraded["workspaces"]["brain"]["env"]["twilio_auth_token"],
            "keep-me"
        );
    }

    #[test]
    fn v3_leaves_an_already_hoisted_machine_value_alone() {
        let mut registry = v3(&json!({"brain": record(&json!({}))}));
        registry["env"] = json!({"markdown_to_pdf_path": "/global/bin"});

        let upgraded = upgrade_to_current(&registry).expect("older registry upgrades");

        assert_eq!(upgraded["env"]["markdown_to_pdf_path"], "/global/bin");
        assert!(
            upgraded["env"].get("brain_receiver_public_url").is_none(),
            "a machine that never set an origin gains no empty one"
        );
    }
}
