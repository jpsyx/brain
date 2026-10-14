mod process;
mod protocol;

fn request() -> super::protocol::Request {
    super::protocol::Request {
        schema_version: 1,
        operation_id: "operation-1".into(),
        action_id: "action-1".into(),
        workspace_id: "workspace-1".into(),
        actor_id: "member".into(),
        action: "cancel".into(),
        issue: "TRACK-1".into(),
    }
}
