mod runner_support;
use runner_support::*;

fn run_late_revocation(revocation: LateRevocation) {
    let provider_id = provider_id(revocation);
    let fixture = tempfile::tempdir().expect("receiver fixture");
    let workspace_id = WorkspaceId::parse(PERSONAL_ID).expect("workspace ID");
    let workspace_name = WorkspaceName::parse("personal").expect("workspace name");
    let workspace_root = fixture.path().join("personal");
    let manifest = crate::workspace::WorkspaceManifest::new(workspace_id);
    let ingress = IngressId::from(manifest.receiver_ingress_id());
    manifest
        .write_new(&workspace_root)
        .expect("workspace manifest");
    let workspace = WorkspaceContext::new(
        fixture.path(),
        workspace_id,
        workspace_name.clone(),
        &workspace_root,
        "personal-member",
        fixture.path(),
    )
    .expect("workspace context");
    crate::users::UsersStore::save(
        &workspace,
        &crate::users::Users {
            schema_version: crate::users::USERS_SCHEMA_VERSION,
            users: vec![crate::users::User {
                id: crate::users::UserId::parse("personal-member").expect("user ID"),
                name: "Personal member".to_owned(),
                phones: vec![crate::users::PhoneIdentity {
                    value: "+12125550100".to_owned(),
                    inbound_allowed: true,
                }],
                emails: Vec::new(),
                response_email: None,
            }],
        },
    )
    .expect("portable users");
    let store = RegistryStore::from_path(fixture.path().join("env.json"));
    store
        .replace(&MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: workspace_name.clone(),
            workspaces: BTreeMap::from([(
                workspace_name.clone(),
                WorkspaceRecord {
                    workspace_id,
                    root: workspace_root,
                    aliases: BTreeSet::new(),
                    local_user_id: "personal-member".to_owned(),
                    receiver_enabled: true,
                    env: serde_json::Map::from_iter([
                        (
                            "twilio_auth_token".to_owned(),
                            serde_json::json!("personal-token"),
                        ),
                        (
                            "brain_receiver_public_url".to_owned(),
                            serde_json::json!("https://receiver.example.test"),
                        ),
                    ]),
                },
            )]),
        })
        .expect("machine registry");
    let _guard = crate::tui::singleton::Guard::acquire(&workspace).expect("TUI singleton");
    let socket = crate::tui::singleton::JobSocket::bind(&workspace).expect("job socket");
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let lease_id = LeaseId::new();
    let mut server = ControlServer::new(generation, store.clone(), fixture.path().to_path_buf());
    let registration = LeaseRegistration {
        generation,
        lease_id,
        workspace_id,
        canonical_name: workspace_name.as_str().to_owned(),
        ingress_id: ingress,
        tui_pid: std::process::id(),
        resolved_root: workspace.root().to_path_buf(),
        job_socket: workspace.paths().job_socket(),
    };
    assert!(matches!(
        server.apply(ControlRequest::Register(registration), now),
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
    let (ticket, loader) = server
        .begin_workspace_route(ingress, now)
        .expect("route ticket");
    let context =
        crate::server::workspace_route::WorkspaceContextLoader::load(&loader, ticket.lease())
            .expect("route context");
    let route = server
        .finish_workspace_route(&ticket, context, now)
        .expect("resolved route");
    let control = Arc::new(Mutex::new(server));
    let body = format!("Body=late+disable&From=%2B12125550100&MessageSid={provider_id}");
    let fields = BTreeMap::from([
        ("Body".to_owned(), "late disable".to_owned()),
        ("From".to_owned(), "+12125550100".to_owned()),
        ("MessageSid".to_owned(), provider_id.to_owned()),
    ]);
    let signature = crate::server::security::twilio_signature(
        "personal-token",
        &format!("https://receiver.example.test/w/{ingress}/sms"),
        &fields,
    );
    let wire = format!(
        "POST /w/{ingress}/sms HTTP/1.1\r\nHost: localhost\r\nX-Twilio-Signature: {signature}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let (mut request_client, request_server) = tcp_pair();
    request_client
        .write_all(wire.as_bytes())
        .expect("write signed request");
    let request = crate::server::http::Request::read(request_server).expect("parse request");
    let (provider_finished_tx, provider_finished_rx) = mpsc::sync_channel(1);
    let (release_admission_tx, release_admission_rx) = mpsc::sync_channel(1);
    let (commit_intent_reloaded_tx, commit_intent_reloaded_rx) = mpsc::sync_channel(1);
    let (release_commit_intent_tx, release_commit_intent_rx) = mpsc::sync_channel(1);
    let worker_release_admission = Arc::new(Mutex::new(release_admission_rx));
    let worker_release_commit_intent = Arc::new(Mutex::new(release_commit_intent_rx));
    let worker_control = Arc::clone(&control);
    let clock_control = Arc::clone(&control);
    let commit_probe_control = Arc::clone(&control);
    let injected_now = Arc::new(Mutex::new(now));
    let worker_now = Arc::clone(&injected_now);
    let main_holds_control = Arc::new(AtomicBool::new(false));
    let worker_main_holds_control = Arc::clone(&main_holds_control);
    let clock_sample_count = Arc::new(AtomicUsize::new(0));
    let worker_clock_sample_count = Arc::clone(&clock_sample_count);
    let intent_reload_count = Arc::new(AtomicUsize::new(0));
    let worker_intent_reload_count = Arc::clone(&intent_reload_count);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut request = request;
        let admission_clock = match revocation {
            LateRevocation::ExpireBeforeCommitWithoutWatchdog => Some({
                let instants = Arc::new(Mutex::new(std::collections::VecDeque::from([
                    now,
                    now + crate::server::lifecycle::LEASE_TTL,
                ])));
                Arc::new(move || {
                    instants
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .pop_front()
                        .unwrap_or(now + crate::server::lifecycle::LEASE_TTL)
                }) as Arc<dyn Fn() -> Instant + Send + Sync>
            }),
            LateRevocation::ExpireDuringCommitIntentReload => Some(Arc::new(move || {
                *worker_now
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            })
                as Arc<dyn Fn() -> Instant + Send + Sync>),
            LateRevocation::ExpireWhileCommitWaitsForControl => Some(Arc::new(move || {
                let sample = worker_clock_sample_count.fetch_add(1, Ordering::AcqRel);
                if sample == 0
                    || worker_main_holds_control.load(Ordering::Acquire)
                    || clock_control.try_lock().is_ok()
                {
                    now
                } else {
                    *worker_now
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                }
            })
                as Arc<dyn Fn() -> Instant + Send + Sync>),
            _ => None,
        };
        let after_final_intent_reload = matches!(
            revocation,
            LateRevocation::ExpireDuringCommitIntentReload
                | LateRevocation::ExpireWhileCommitWaitsForControl
        )
        .then(|| {
            Arc::new(move || {
                if worker_intent_reload_count.fetch_add(1, Ordering::AcqRel) == 1 {
                    commit_intent_reloaded_tx
                        .send(())
                        .expect("signal commit-side intent reload");
                    worker_release_commit_intent
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .recv_timeout(Duration::from_secs(1))
                        .expect("release commit-side intent reload");
                }
            }) as Arc<dyn Fn() + Send + Sync>
        });
        let mut pipeline = SharedReceiverPipeline {
            route: Some(route),
            request: &mut request,
            control: &worker_control,
            channel: Channel::Sms,
            handoff_deadline: None,
            admission: None,
            admission_clock,
            after_final_intent_reload,
            after_combined_commit: matches!(
                revocation,
                LateRevocation::CommitLinearizesUnderControl
            )
            .then(|| {
                Arc::new(move |admission: &ReceiverAdmission| {
                    assert!(
                        commit_probe_control.try_lock().is_err(),
                        "control mutex was released before admission commit linearized"
                    );
                    assert!(
                        admission.is_committed(),
                        "admission CAS had not committed inside the control mutex"
                    );
                }) as Arc<dyn Fn(&ReceiverAdmission) + Send + Sync>
            }),
            before_final_admission: Some(Box::new(move || {
                provider_finished_tx
                    .send(())
                    .expect("signal final revalidation");
                worker_release_admission
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .recv_timeout(Duration::from_secs(1))
                    .expect("release final socket admission");
            })),
        };
        result_tx
            .send(execute_pipeline(&mut pipeline))
            .expect("report dispatch result");
    });

    let queue = Arc::new(Mutex::new(crate::tui::receiver::InboundQueue::default()));
    let stop_polling = Arc::new(AtomicBool::new(false));
    let poller_queue = Arc::clone(&queue);
    let poller_stop = Arc::clone(&stop_polling);
    let poller = std::thread::spawn(move || {
        while !poller_stop.load(Ordering::Acquire) {
            socket.poll_jobs(
                workspace_id,
                &mut poller_queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            std::thread::yield_now();
        }
    });

    provider_finished_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("pipeline reached final revalidation");
    match revocation {
        LateRevocation::Disable => {
            store
                .transition_receiver(
                    &workspace_name,
                    workspace_id,
                    crate::workspace::ReceiverAction::Stop,
                )
                .expect("persist disable");
            let response = control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .apply(
                    ControlRequest::RefreshEnabled {
                        generation,
                        workspace_id,
                    },
                    Instant::now(),
                );
            assert!(matches!(response, ControlResponse::Accepted { .. }));
        }
        LateRevocation::Unregister => {
            let response = control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .apply(
                    ControlRequest::Unregister {
                        generation,
                        lease_id,
                    },
                    Instant::now(),
                );
            assert!(matches!(response, ControlResponse::Accepted { .. }));
        }
        LateRevocation::DisableEnableAba => {
            store
                .transition_receiver(
                    &workspace_name,
                    workspace_id,
                    crate::workspace::ReceiverAction::Stop,
                )
                .expect("persist disable");
            let response = control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .apply(
                    ControlRequest::RefreshEnabled {
                        generation,
                        workspace_id,
                    },
                    Instant::now(),
                );
            assert!(matches!(response, ControlResponse::Accepted { .. }));
            store
                .transition_receiver(
                    &workspace_name,
                    workspace_id,
                    crate::workspace::ReceiverAction::Start,
                )
                .expect("persist re-enable");
            let response = control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .apply(
                    ControlRequest::RefreshEnabled {
                        generation,
                        workspace_id,
                    },
                    Instant::now(),
                );
            assert!(matches!(response, ControlResponse::Accepted { .. }));
        }
        LateRevocation::Expire | LateRevocation::RouteLookupThenExpire => {
            let expiry = now + crate::server::lifecycle::LEASE_TTL;
            if matches!(revocation, LateRevocation::RouteLookupThenExpire) {
                control
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .begin_workspace_route(ingress, expiry)
                    .expect_err("expired route lookup must be unavailable");
            }
            let decision = ControlServer::expire_shared_until(
                &control,
                expiry,
                Instant::now() + Duration::from_secs(1),
                &Instant::now,
            )
            .expect("watchdog expiry transition");
            assert_eq!(
                decision,
                crate::server::lifecycle::ServerDecision::ShutdownNow
            );
        }
        LateRevocation::ExpireBeforeCommitWithoutWatchdog
        | LateRevocation::CommitLinearizesUnderControl => {}
        LateRevocation::ExpireDuringCommitIntentReload => {
            release_admission_tx
                .send(())
                .expect("release authorize-side socket admission");
            commit_intent_reloaded_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("pipeline reached commit-side intent boundary");
            *injected_now
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                now + crate::server::lifecycle::LEASE_TTL;
            release_commit_intent_tx
                .send(())
                .expect("release expired commit-side intent boundary");
        }
        LateRevocation::ExpireWhileCommitWaitsForControl => {
            main_holds_control.store(true, Ordering::Release);
            let control_guard = control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            release_admission_tx
                .send(())
                .expect("release authorize-side socket admission");
            commit_intent_reloaded_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("pipeline completed commit-side persisted-intent IO");
            release_commit_intent_tx
                .send(())
                .expect("release commit worker toward the held control mutex");
            *injected_now
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                now + crate::server::lifecycle::LEASE_TTL;
            main_holds_control.store(false, Ordering::Release);
            drop(control_guard);
        }
    }
    if !matches!(
        revocation,
        LateRevocation::ExpireDuringCommitIntentReload
            | LateRevocation::ExpireWhileCommitWaitsForControl
    ) {
        release_admission_tx
            .send(())
            .expect("release final socket admission");
    }

    finish_pipeline(revocation, result_rx, stop_polling, poller, queue);
}
