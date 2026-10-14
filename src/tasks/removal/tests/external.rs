use super::fixture::{WorkspaceFixture, needles};
use crate::tasks::removal::{apply, model::Options, operation_status, preview, resume};
use serde_json::{Value, json};

pub(super) fn fixture(body: &str) -> WorkspaceFixture {
    use std::os::unix::fs::PermissionsExt;
    let f = WorkspaceFixture::new();
    f.write(
        "tasks/tasks.csv",
        b"task_id,task_name,status,linear_issue\nT17,First,active,EX-1\nT18,Second,active,EX-2\n",
    );
    let executable = f.temporary.path().join("adapter");
    std::fs::write(&executable, format!(r"#!/usr/bin/python3
import json,sys,pathlib
r=json.load(sys.stdin)
base=pathlib.Path(__file__).parent
with (base/'calls').open('a') as log: log.write(r['action']+' '+r['issue']+'\n')
assert r['workspace_id']=='e806258e-491a-436d-9db4-a5ca9903e0d4'
assert r['actor_id']=='member'
if r['action']=='cancel':
    directory=pathlib.Path((base/'operation-directory').read_text())
    saved=json.loads((directory/(r['operation_id']+'.json')).read_text())
    action=next(a for a in saved['external'] if a['action_id']==r['action_id'])
    assert action['issue']==r['issue'] and action['state']=='unknown'
    assert saved['phase']=='external_pending'
status='ready' if r['action']=='preflight' else 'canceled'
{body}
print(json.dumps(dict(schema_version=1,operation_id=r['operation_id'],action_id=r['action_id'],status=status,message='provider diagnostic')))
")).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        f.temporary.path().join("operation-directory"),
        f.workspace
            .paths()
            .task_removals_dir()
            .to_string_lossy()
            .as_bytes(),
    )
    .unwrap();
    super::integration_config::policy(&f);
    super::integration_config::registry(
        &f,
        json!({"tracker":{"argv":[executable],"env_names":[],"timeout_seconds":30}}),
    );
    f
}

pub(super) fn start(
    f: &WorkspaceFixture,
) -> Result<crate::tasks::removal::operations::Receipt, crate::tasks::removal::model::Problem> {
    let date = chrono::Local::now().date_naive();
    let context = f.command();
    let plan = preview(
        &context,
        &needles(&["T17", "T18"]),
        Options::default(),
        date,
    )
    .unwrap();
    apply(&context, &plan, &plan.fingerprint, date)
}

pub(super) fn calls(f: &WorkspaceFixture) -> String {
    std::fs::read_to_string(f.temporary.path().join("calls")).unwrap_or_default()
}

pub(super) fn rows(f: &WorkspaceFixture) -> String {
    std::fs::read_to_string(f.workspace.root().join("tasks/tasks.csv")).unwrap()
}

pub(super) fn progress(f: &WorkspaceFixture, id: &str) -> Value {
    serde_json::to_value(operation_status(&f.command(), id).unwrap()).unwrap()
}

#[test]
fn removal_external_resume_retains_local_rows_and_never_repeats_confirmed_cancel() {
    let f = fixture(
        "if r['action']=='cancel' and r['issue']=='EX-2' and not (base/'retry').exists(): status='error'\nif r['action']=='status': status='pending'",
    );
    let before = rows(&f);
    let error = start(&f).unwrap_err();
    assert_eq!(error.code, "external_pending");
    let id = error.details["operation_id"].as_str().unwrap();
    assert_eq!(rows(&f), before);
    assert_eq!(progress(&f, id)["phase"], "external_pending");
    assert_eq!(progress(&f, id)["external"][0]["state"], "succeeded");
    assert_eq!(progress(&f, id)["external"][1]["state"], "unknown");
    assert_eq!(
        calls(&f),
        "preflight EX-1\npreflight EX-2\ncancel EX-1\ncancel EX-2\n"
    );
    std::fs::write(f.temporary.path().join("retry"), b"retry").unwrap();
    let receipt = resume(&f.command(), id).unwrap();
    assert!(receipt.committed);
    assert_eq!(rows(&f).lines().count(), 1);
    assert_eq!(calls(&f).matches("cancel EX-1\n").count(), 1);
    assert!(calls(&f).contains("status EX-2\n"));
}
