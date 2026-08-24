use std::time::Instant;

use brain::workspace::WorkspaceContext;

pub(super) fn register(
    client: &brain::server::control::ServerClient,
    generation: brain::server::lifecycle::ServerGeneration,
    workspace: &WorkspaceContext,
    ingress_id: brain::server::IngressId,
) -> brain::server::control::HeartbeatWorker {
    let lease_id = brain::server::lifecycle::LeaseId::new();
    let registration = brain::server::control::LeaseRegistration {
        generation,
        lease_id,
        workspace_id: workspace.id(),
        canonical_name: workspace.name().as_str().to_owned(),
        ingress_id,
        tui_pid: std::process::id(),
        resolved_root: workspace.root().to_path_buf(),
        job_socket: workspace.paths().job_socket(),
    };
    client.register_generation(&registration).unwrap();
    brain::server::control::HeartbeatWorker::start(client.clone(), registration)
}

pub(super) fn poll_value<T>(deadline: Instant, mut value: impl FnMut() -> Option<T>) -> T {
    loop {
        if let Some(value) = value() {
            return value;
        }
        assert!(Instant::now() < deadline, "value was not produced");
        std::thread::yield_now();
    }
}
