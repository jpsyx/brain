use brain::server::control::{HeartbeatClock, HeartbeatEvent, HeartbeatWorker, RegistrationGate};
use brain::server::lifecycle::{ProcessRecord, ServerDecision};
use std::sync::{Arc, Barrier, mpsc::Receiver};

use super::support::{LiveTui, RunningServer, wait_for};

#[test]
fn two_tui_heartbeats_race_recovery_and_share_one_replacement_generation() {
    let mut server = RunningServer::start();
    let family = LiveTui::new(
        server.home(),
        "family",
        "e806258e-491a-436d-9db4-a5ca9903e0d4",
        "57b162df-983a-45c3-ac7e-bad94eb27a99",
        "00000000-0000-0000-0000-000000000011",
    );
    let personal = LiveTui::new(
        server.home(),
        "personal",
        "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        "00000000-0000-0000-0000-000000000012",
    );
    let family_registration = family.registration(server.generation);
    let personal_registration = personal.registration(server.generation);
    server
        .client
        .register_generation(&family_registration)
        .expect("register family");
    server
        .client
        .register_generation(&personal_registration)
        .expect("register personal");
    server.child.kill().expect("crash shared server");
    server.child.wait().expect("reap crashed server");
    let recovery_boundary = Arc::new(Barrier::new(3));
    let mut family_worker = HeartbeatWorker::start_with_clock(
        server.client.clone(),
        family_registration,
        BarrierClock::new(Arc::clone(&recovery_boundary)),
    );
    let mut personal_worker = HeartbeatWorker::start_with_clock(
        server.client.clone(),
        personal_registration,
        BarrierClock::new(Arc::clone(&recovery_boundary)),
    );
    recovery_boundary.wait();
    let mut family_recovered = None;
    let mut personal_recovered = None;
    wait_for("both TUI leases to recover", || {
        for event in family_worker.poll() {
            if let HeartbeatEvent::Recovered(generation) = event {
                family_recovered = Some(generation);
            }
        }
        for event in personal_worker.poll() {
            if let HeartbeatEvent::Recovered(generation) = event {
                personal_recovered = Some(generation);
            }
        }
        family_recovered.is_some() && personal_recovered.is_some()
    });

    assert_eq!(family_recovered, personal_recovered);
    assert_ne!(family_recovered, Some(server.generation));
    family_worker.shutdown().expect("unregister family");
    personal_worker.shutdown().expect("unregister personal");
    wait_for("replacement generation cleanup", || {
        !server.paths.process_record().exists()
            && !server.paths.control_socket().exists()
            && !server.paths.election_lock().exists()
    });
}

#[test]
fn startup_registration_recovers_when_final_unregister_wins_after_connect() {
    let mut server = RunningServer::start();
    let personal = LiveTui::new(
        server.home(),
        "personal",
        "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        "00000000-0000-0000-0000-000000000021",
    );
    let family = LiveTui::new(
        server.home(),
        "family",
        "e806258e-491a-436d-9db4-a5ca9903e0d4",
        "57b162df-983a-45c3-ac7e-bad94eb27a99",
        "00000000-0000-0000-0000-000000000022",
    );
    server
        .client
        .register_generation(&personal.registration(server.generation))
        .expect("register personal");
    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let gate = FirstConnectGate::new(Arc::clone(&reached), Arc::clone(&release));
    let client = server.client.clone();
    let mut registration = family.registration(server.generation);
    let handshake = std::thread::spawn(move || {
        let record = client
            .connect_and_register_with_gate(&mut registration, gate)
            .expect("recover startup registration");
        (record, registration)
    });
    reached.wait();

    assert_eq!(
        server
            .client
            .unregister_generation(server.generation, personal.lease_id)
            .expect("final unregister"),
        ServerDecision::ShutdownNow
    );
    wait_for("old generation exit before registration", || {
        server.child.try_wait().ok().flatten().is_some()
    });
    release.wait();
    let (replacement, registration) = handshake.join().expect("registration worker");

    assert_ne!(replacement.generation, server.generation);
    assert_eq!(registration.generation, replacement.generation);
    server
        .client
        .unregister_generation(replacement.generation, family.lease_id)
        .expect("unregister family replacement");
    wait_for("replacement generation cleanup", || {
        !server.paths.process_record().exists()
            && !server.paths.control_socket().exists()
            && !server.paths.election_lock().exists()
    });
}

struct BarrierClock {
    recovery_boundary: Arc<Barrier>,
    ticked: bool,
}

impl BarrierClock {
    fn new(recovery_boundary: Arc<Barrier>) -> Self {
        Self {
            recovery_boundary,
            ticked: false,
        }
    }
}

impl HeartbeatClock for BarrierClock {
    fn wait_for_tick(&mut self, _stop: &Receiver<()>) -> bool {
        if self.ticked {
            return false;
        }
        self.ticked = true;
        true
    }

    fn recovery_boundary(&mut self) {
        self.recovery_boundary.wait();
    }
}

struct FirstConnectGate {
    reached: Arc<Barrier>,
    release: Arc<Barrier>,
    blocked: bool,
}

impl FirstConnectGate {
    fn new(reached: Arc<Barrier>, release: Arc<Barrier>) -> Self {
        Self {
            reached,
            release,
            blocked: false,
        }
    }
}

impl RegistrationGate for FirstConnectGate {
    fn after_connect(&mut self, _record: &ProcessRecord) {
        if self.blocked {
            return;
        }
        self.blocked = true;
        self.reached.wait();
        self.release.wait();
    }
}
