use std::time::{Duration, Instant};

use super::*;
use crate::server::lifecycle::ServerGeneration;
use crate::workspace::RegistryStore;

#[test]
fn workspace_status_reports_a_live_but_disabled_exact_lease() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let store = RegistryStore::from_path(fixture.path().join("env.json"));
    store
        .replace(&registry_with_receiver(true))
        .expect("persist enabled receiver intent");
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(generation, store, fixture.path().to_path_buf());
    let mut disabled_lease = lease(now + Duration::from_secs(30));
    disabled_lease.receiver_enabled = false;
    server
        .leases
        .register(disabled_lease, now)
        .expect("register disabled fixture lease");

    assert_eq!(
        server.apply(
            ControlRequest::WorkspaceStatus {
                generation,
                workspace_id: workspace_id(),
            },
            now,
        ),
        ControlResponse::WorkspaceStatus {
            generation,
            receiver_enabled: Some(false),
        }
    );
}
