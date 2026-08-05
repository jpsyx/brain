use std::sync::Arc;

use brain::actor::{RequestIdentity, resolve_actor};
use brain::users::{USERS_SCHEMA_VERSION, User, UserId, Users};
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

pub(crate) fn family_id() -> WorkspaceId {
    WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("workspace id")
}

pub(crate) fn temporary_workspace() -> (tempfile::TempDir, Arc<WorkspaceContext>) {
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join("family");
    std::fs::create_dir_all(root.join(".config")).expect("workspace config directory");
    let workspace = WorkspaceContext::new(
        home.path(),
        family_id(),
        WorkspaceName::parse("family").expect("workspace name"),
        &root,
        "pablo",
        home.path(),
    )
    .expect("workspace context");
    (home, Arc::new(workspace))
}

pub(crate) fn actor() -> brain::actor::ActorContext {
    named_actor("pablo", "Pablo")
}

pub(crate) fn named_actor(id: &str, name: &str) -> brain::actor::ActorContext {
    let id = UserId::parse(id).expect("user id");
    resolve_actor(
        &id,
        RequestIdentity::Local,
        &Users {
            schema_version: USERS_SCHEMA_VERSION,
            users: vec![User {
                id: id.clone(),
                name: name.to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            }],
        },
    )
    .expect("actor")
}
