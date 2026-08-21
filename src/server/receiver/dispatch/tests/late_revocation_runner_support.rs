use super::*;

pub(super) fn provider_id(revocation: LateRevocation) -> &'static str {
    match revocation {
        LateRevocation::Disable => "SM-late-disable",
        LateRevocation::Unregister => "SM-late-unregister",
        LateRevocation::DisableEnableAba => "SM-late-aba",
        LateRevocation::Expire => "SM-late-expire",
        LateRevocation::RouteLookupThenExpire => "SM-route-prune-expire",
        LateRevocation::ExpireBeforeCommitWithoutWatchdog => "SM-expire-before-commit",
        LateRevocation::ExpireDuringCommitIntentReload => "SM-expire-during-intent-reload",
        LateRevocation::ExpireWhileCommitWaitsForControl => "SM-expire-waiting-for-control",
        LateRevocation::CommitLinearizesUnderControl => "SM-commit-under-control",
    }
}

pub(super) fn finish_pipeline(
    revocation: LateRevocation,
    result_rx: std::sync::mpsc::Receiver<anyhow::Result<InboundJob>>,
    stop_polling: std::sync::Arc<std::sync::atomic::AtomicBool>,
    poller: std::thread::JoinHandle<()>,
    queue: std::sync::Arc<std::sync::Mutex<crate::tui::receiver::InboundQueue>>,
) {
    let deadline = Instant::now() + Duration::from_secs(1);
    let result = loop {
        if let Ok(result) = result_rx.try_recv() {
            break result;
        }
        assert!(Instant::now() < deadline, "shared pipeline did not finish");
        std::thread::yield_now();
    };
    stop_polling.store(true, Ordering::Release);
    poller.join().expect("job socket poller");
    if matches!(revocation, LateRevocation::CommitLinearizesUnderControl) {
        result.expect("live authority should commit under the control mutex");
        assert_eq!(
            queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            1,
            "committed work did not reach the real live-TUI queue"
        );
    } else {
        result.expect_err("revoked authority must reject before the real job-socket handoff");
        assert!(
            queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty(),
            "revoked work reached the live TUI queue"
        );
    }
}

fn tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test listener");
    let client = TcpStream::connect(listener.local_addr().expect("test address"))
        .expect("connect request client");
    let (server, _) = listener.accept().expect("accept request client");
    (client, server)
}
