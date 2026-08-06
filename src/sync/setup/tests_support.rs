use std::cell::RefCell;

use super::*;

const LOCAL_WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const OTHER_WORKSPACE_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

fn local_workspace_id() -> crate::workspace::WorkspaceId {
    crate::workspace::WorkspaceId::parse(LOCAL_WORKSPACE_ID).expect("fixed workspace UUID")
}

