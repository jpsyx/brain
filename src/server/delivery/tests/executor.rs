use super::super::*;

#[test]
fn bounded_executor_returns_a_typed_result_without_running_on_the_caller() {
    let executor = BoundedDeliveryExecutor::<u8, &'static str>::new(1, "test-delivery")
        .expect("bounded executor");
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
    let (finish_tx, finish_rx) = std::sync::mpsc::sync_channel(0);
    let permit = executor
        .reserve(7, move || {
            started_tx.send(()).expect("signal worker start");
            finish_rx.recv().expect("release worker");
            "acknowledged"
        })
        .expect("reserve work");

    permit.start().expect("publish reserved work");
    started_rx.recv().expect("work started off caller");
    assert_eq!(executor.poll(), DeliveryExecutorPoll::Pending);
    finish_tx.send(()).expect("finish worker");
    let result = loop {
        if let DeliveryExecutorPoll::Ready(result) = executor.poll() {
            break result;
        }
        std::thread::yield_now();
    };
    assert_eq!(result.input, 7);
    assert_eq!(result.output, "acknowledged");
}

#[test]
fn queue_saturation_is_reported_before_provider_work_can_start() {
    let executor =
        BoundedDeliveryExecutor::<u8, u8>::new(1, "test-saturation").expect("bounded executor");
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
    let (finish_tx, finish_rx) = std::sync::mpsc::sync_channel(0);
    executor
        .reserve(1, move || {
            started_tx.send(()).expect("signal first start");
            finish_rx.recv().expect("release first work");
            1
        })
        .expect("reserve first")
        .start()
        .expect("start first");
    started_rx.recv().expect("first work active");

    let second = executor.reserve(2, || 2).expect("reserve queued work");
    let saturated = executor
        .reserve(3, || 3)
        .expect_err("third reservation exceeds the exact bound");
    assert_eq!(saturated.into_input(), 3);

    finish_tx.send(()).expect("finish first");
    second.start().expect("start second");
}

#[test]
fn publishing_queued_work_never_waits_for_the_worker_on_the_caller() {
    let executor = BoundedDeliveryExecutor::<u8, u8>::new(1, "test-nonblocking-publish")
        .expect("bounded executor");
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
    let (finish_tx, finish_rx) = std::sync::mpsc::sync_channel(0);
    executor
        .reserve(1, move || {
            started_tx.send(()).expect("signal active work");
            finish_rx.recv().expect("release active work");
            1
        })
        .expect("reserve active work")
        .start()
        .expect("publish active work");
    started_rx.recv().expect("active work started");
    let queued = executor.reserve(2, || 2).expect("reserve queued work");
    let (published_tx, published_rx) = std::sync::mpsc::sync_channel(1);
    let publish = std::thread::spawn(move || {
        published_tx
            .send(queued.start().is_ok())
            .expect("record publication result");
    });

    let immediate = published_rx.recv_timeout(std::time::Duration::from_millis(250));
    finish_tx.send(()).expect("finish active work");
    if immediate.is_err() {
        published_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("blocked publication eventually released");
    }
    publish.join().expect("join publication probe");

    assert_eq!(
        immediate,
        Ok(true),
        "event-loop publication must be nonblocking"
    );
}

#[test]
fn dropped_reservation_never_executes_provider_work() {
    let executor =
        BoundedDeliveryExecutor::<u8, u8>::new(1, "test-cancel").expect("bounded executor");
    let executed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = executed.clone();
    let permit = executor
        .reserve(1, move || {
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            1
        })
        .expect("reserve work");

    drop(permit);
    for _ in 0..100 {
        if executor.poll() == DeliveryExecutorPoll::Disconnected {
            break;
        }
        std::thread::yield_now();
    }
    assert!(!executed.load(std::sync::atomic::Ordering::SeqCst));
}
