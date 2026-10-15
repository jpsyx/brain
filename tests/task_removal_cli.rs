use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use brain::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceId, WorkspaceManifest,
    WorkspaceName, WorkspaceRecord,
};
use serde_json::{Value, json};

struct Fixture {
    temp: tempfile::TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        for name in [
            "tasks",
            "projects",
            "areas",
            "resources",
            "archive",
            ".config",
        ] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        let id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
        WorkspaceManifest::new(id).write_new(&root).unwrap();
        std::fs::write(root.join(".config/users.json"), r#"{"schema_version":1,"users":[{"id":"tester","name":"Tester","phones":[],"emails":[],"response_email":null}]}"#).unwrap();
        let name = WorkspaceName::parse("sample").unwrap();
        let registry = MachineRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            default_workspace: name.clone(),
            workspaces: BTreeMap::from([(
                name,
                WorkspaceRecord {
                    workspace_id: id,
                    root: root.clone(),
                    aliases: BTreeSet::new(),
                    local_user_id: "tester".into(),
                    receiver_enabled: false,
                    env: serde_json::from_value(json!({
                        "agenda_markdown_dir": temp.path().join("agendas"),
                        "markdown_to_pdf_path": "/usr/bin/true"
                    }))
                    .unwrap(),
                },
            )]),
            env: serde_json::from_value(json!({"markdown_to_pdf_path": "/usr/bin/true"})).unwrap(),
        };
        RegistryStore::from_path(temp.path().join("config/brain/env.json"))
            .replace(&registry)
            .unwrap();
        let fixture = Self { temp, root };
        let warm = fixture.run(&["tasks", "linked", "--json"]);
        assert!(
            warm.status.success(),
            "{}",
            String::from_utf8_lossy(&warm.stderr)
        );
        fixture.seed("T1,First,not_started,,,2026-10-14,\nT2,Second,not_started,,,2026-10-14,\n");
        std::fs::create_dir_all(fixture.root.join("projects/sample__outcome")).unwrap();
        std::fs::write(fixture.root.join("projects/sample__outcome/.METADATA.json"), serde_json::to_vec(&json!({
            "name": "sample__outcome", "namespace": "sample", "title": "Example outcome", "status": "in-progress",
            "priority": "p2", "due": "none", "directory": "projects/sample__outcome", "tasks": ["T1", "T2"], "extra_reference": "preserve"
        })).unwrap()).unwrap();
        fixture
    }
    fn seed(&self, rows: &str) {
        std::fs::write(
            self.root.join("tasks/tasks.csv"),
            format!(
                "task_id,task_name,status,blocked_by,linear_issue,last_touched,system_key\n{rows}"
            ),
        )
        .unwrap();
        std::fs::write(self.root.join("tasks/habits.csv"), "task_id,task_name,status,blocked_by,linear_issue,last_touched,system_key\nH10,Recurring,not_started,,,2026-10-14,\nH20,Triage,not_started,,,2026-10-14,brain.triage.daily\n").unwrap();
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_brain"));
        cmd.current_dir(self.temp.path())
            .env("HOME", self.temp.path().join("home"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("XDG_CACHE_HOME", self.temp.path().join("cache"))
            .env("NO_COLOR", "1")
            .env_remove("BRAIN_TASKS_CSV")
            .env_remove("BRAIN_WORKSPACE")
            .env_remove("BRAIN_ROOT")
            .env_remove("BRAIN_WORKSPACE_ID")
            .env_remove("BRAIN_REQUIRE_WORKSPACE")
            .args(["-w", "sample"])
            .stdin(Stdio::null());
        cmd
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn bytes(&self) -> Vec<Vec<u8>> {
        [
            "tasks/tasks.csv",
            "tasks/habits.csv",
            "tasks/SCHEMA.md",
            "projects/sample__outcome/.METADATA.json",
        ]
        .map(|p| std::fs::read(self.root.join(p)).unwrap_or_default())
        .into()
    }
}
fn document(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON stdout document");
    assert_eq!(value["schema_version"], 1);
    let text = value.to_string();
    for hidden in ["canonical_inputs", "before", "after", "edits"] {
        assert!(!text.contains(&format!("\"{hidden}\"")), "{text}");
    }
    value
}
#[test]
fn removal_nonterminal_requires_consent_and_json_is_noninteractive() {
    for flags in [vec![], vec!["--json"]] {
        let f = Fixture::new();
        let before = f.bytes();
        let mut args = vec!["tasks", "remove", "T1"];
        args.extend(flags);
        if args.contains(&"--json") {
            args.insert(0, "--verbose");
        }
        let out = f.run(&args);
        if args.contains(&"--json") {
            assert_eq!(document(&out, 2)["code"], "confirmation_required");
        } else {
            assert_eq!(out.status.code(), Some(2));
            assert!(out.stdout.is_empty());
            assert!(String::from_utf8_lossy(&out.stderr).contains("confirmation_required"));
        }
        assert_eq!(f.bytes(), before);
    }
}
#[test]
fn removal_alias_batches_preview_without_mutation_then_apply() {
    for alias in ["remove", "rm", "drop"] {
        let f = Fixture::new();
        let before = f.bytes();
        let preview = document(
            &f.run(&[
                "--verbose",
                "tasks",
                alias,
                "t2",
                "1",
                "T2",
                "--dry-run",
                "--json",
            ]),
            0,
        );
        assert_eq!(preview["kind"], "preview");
        assert_eq!(preview["ready"], true);
        assert_eq!(preview["targets"][0]["task_id"], "T1");
        assert_eq!(preview["targets"][1]["task_id"], "T2");
        assert_eq!(preview["targets"].as_array().unwrap().len(), 2);
        assert_eq!(f.bytes(), before);
        let result = document(
            &f.run(&[
                "tasks",
                alias,
                "T2",
                "T1",
                "--yes",
                "--json",
                "--expect-plan",
                preview["fingerprint"].as_str().unwrap(),
            ]),
            0,
        );
        assert_eq!(result["kind"], "result");
        assert_eq!(result["committed"], true);
        assert_eq!(result["removed"].as_array().unwrap().len(), 2);
        let op = result["operation_id"].as_str().unwrap();
        let status = document(
            &f.run(&["tasks", "remove", "--operation-status", op, "--json"]),
            0,
        );
        assert_eq!(status["kind"], "operation");
        assert_eq!(status["phase"], "complete");
        let after = f.bytes();
        assert_eq!(
            document(&f.run(&["tasks", "remove", "--resume", op, "--json"]), 0)["operation_id"],
            op
        );
        assert_eq!(f.bytes(), after);
        assert_eq!(
            document(
                &f.run(&["tasks", "remove", "--abandon", op, "--yes", "--json"]),
                2
            )["code"],
            "already_committed"
        );
    }
}
#[test]
fn removal_guards_reject_entire_batch_and_preserve_data() {
    for (targets, flags, code) in [
        (vec!["T1", "H10"], vec!["--yes"], "mixed_target_kinds"),
        (
            vec!["H20"],
            vec!["--habit", "--yes"],
            "managed_task_cannot_delete",
        ),
        (vec!["H10"], vec!["--yes"], "habit_required"),
        (vec!["T1", "missing"], vec!["--yes"], "target_not_found"),
        (
            vec!["T1"],
            vec!["--yes", "--expect-plan", "stale"],
            "stale_plan",
        ),
        (
            vec!["T1"],
            vec!["--yes", "--status", "done"],
            "removal_filters_not_supported",
        ),
    ] {
        let f = Fixture::new();
        let before = f.bytes();
        let mut args = vec!["tasks", "remove"];
        args.extend(targets);
        args.extend(flags);
        args.push("--json");
        let error = document(&f.run(&args), 2);
        assert_eq!(error["kind"], "error");
        assert_eq!(error["code"], code);
        assert_eq!(f.bytes(), before);
    }
}
#[test]
fn removal_yes_does_not_choose_a_dependency_policy() {
    let f = Fixture::new();
    f.seed("T1,First,not_started,,,2026-10-14,\nT2,Second,not_started,T1,,2026-10-14,\n");
    let before = f.bytes();
    let preview = document(&f.run(&["tasks", "remove", "T1", "--dry-run", "--json"]), 2);
    assert_eq!(preview["ready"], false);
    assert_eq!(preview["blockers"][0]["code"], "dependents_exist");
    assert_eq!(
        document(&f.run(&["tasks", "remove", "T1", "--yes", "--json"]), 2)["code"],
        "dependents_exist"
    );
    assert_eq!(f.bytes(), before);
    assert_eq!(
        document(
            &f.run(&[
                "tasks",
                "remove",
                "T1",
                "--dependents",
                "unlink",
                "--yes",
                "--json"
            ]),
            0
        )["committed"],
        true
    );
}
#[test]
fn removal_operation_modes_validate_before_lookup() {
    for flags in [
        vec!["--resume", "missing", "--yes"],
        vec!["--operation-status", "missing", "--habit"],
        vec!["--resume", "missing", "--dry-run"],
        vec!["--abandon", "missing"],
    ] {
        let f = Fixture::new();
        let before = f.bytes();
        let mut args = vec!["tasks", "remove", "--json"];
        args.extend(flags);
        assert_eq!(document(&f.run(&args), 2)["code"], "invalid_removal_mode");
        assert_eq!(f.bytes(), before);
    }
    let f = Fixture::new();
    assert_eq!(
        document(
            &f.run(&["tasks", "remove", "--operation-status", "missing", "--json"]),
            1
        )["code"],
        "operation_unavailable"
    );
}
#[test]
fn removal_human_output_and_clap_syntax_keep_streams_and_exits() {
    let f = Fixture::new();
    let out = f.run(&["tasks", "remove", "T1", "--yes"]);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("T1"));
    let out = f.run(&["tasks", "remove", "T2", "--dependents", "invalid", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid value"));
}
#[test]
fn removal_json_setup_failure_is_one_document_even_with_verbose() {
    let f = Fixture::new();
    std::fs::rename(&f.root, f.temp.path().join("unavailable-workspace")).unwrap();
    std::fs::write(&f.root, "not a workspace directory").unwrap();
    let out = f.run(&["--verbose", "tasks", "remove", "T1", "--yes", "--json"]);
    assert_eq!(document(&out, 1)["code"], "command_failed");
}
#[test]
fn removal_status_does_not_repair_readiness_or_start_receiver() {
    let f = Fixture::new();
    let result = document(&f.run(&["tasks", "remove", "T1", "--yes", "--json"]), 0);
    let id = result["operation_id"].as_str().unwrap();
    let users = f.root.join(".config/users.json");
    std::fs::remove_file(&users).unwrap();
    let before = f.bytes();
    let out = f.run(&[
        "--verbose",
        "--with-receiver",
        "tasks",
        "rm",
        "--operation-status",
        id,
        "--json",
    ]);
    assert_eq!(document(&out, 0)["phase"], "complete");
    assert!(!users.exists(), "status must not repair portable users");
    assert_eq!(f.bytes(), before);
}
#[test]
fn removal_commit_does_not_require_a_pdf_renderer() {
    let f = Fixture::new();
    let path = f.temp.path().join("config/brain/env.json");
    let mut registry: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    registry["env"]["markdown_to_pdf_path"] = json!("/nonexistent/task-removal-renderer");
    std::fs::write(&path, serde_json::to_vec(&registry).unwrap()).unwrap();
    let out = f
        .command()
        .env("PATH", "/usr/bin:/bin")
        .env("ZDOTDIR", f.temp.path())
        .args(["tasks", "remove", "T1", "--yes", "--json"])
        .output()
        .unwrap();
    assert_eq!(document(&out, 0)["committed"], true);
    let after: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        after["env"]["markdown_to_pdf_path"], "/nonexistent/task-removal-renderer",
        "removal must skip startup PDF discovery/repair"
    );
}

#[test]
fn removal_status_reads_saved_evidence_without_recovery_or_other_writes() {
    let f = Fixture::new();
    let result = document(&f.run(&["tasks", "remove", "T1", "--yes", "--json"]), 0);
    let id = result["operation_id"].as_str().unwrap();
    let journal = f.temp.path().join("home/.cache/brain/workspaces/8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b/task-removal-transaction.json");
    std::fs::write(journal, "invalid pending journal must remain untouched").unwrap();
    let snapshot = || {
        walkdir::WalkDir::new(f.temp.path())
            .into_iter()
            .map(Result::unwrap)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| {
                (
                    entry
                        .path()
                        .strip_prefix(f.temp.path())
                        .unwrap()
                        .to_path_buf(),
                    std::fs::read(entry.path()).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let before = snapshot();
    assert_eq!(
        document(
            &f.run(&[
                "--verbose",
                "--with-receiver",
                "tasks",
                "drop",
                "--operation-status",
                id,
                "--json"
            ]),
            0
        )["phase"],
        "complete"
    );
    assert_eq!(snapshot(), before);
}
#[test]
fn removal_status_classifier_accepts_aliases_and_normalized_workspace_flags() {
    for alias in ["remove", "rm", "drop"] {
        let cli = brain::cli::try_parse_from([
            "brain",
            "tasks",
            "--status",
            "done",
            alias,
            "--operation-status",
            "id",
            "--json",
            "-w",
            "sample",
        ])
        .unwrap();
        assert_eq!(
            brain::workspace::invocation_for(&cli),
            brain::workspace::Invocation::TaskRemovalStatus
        );
        assert!(brain::workspace::is_read_only_status(&cli));
        assert_eq!(
            brain::workspace::bootstrap_policy(brain::workspace::invocation_for(&cli)),
            brain::workspace::BootstrapPolicy::ReadOnlyWorkspace
        );
    }
}

#[path = "task_removal_cli/canonical.rs"]
mod canonical;
