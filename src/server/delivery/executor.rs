type DeliveryOperation<R> = Box<dyn FnOnce() -> R + Send + 'static>;

struct ReservedDelivery<T, R> {
    input: T,
    start: std::sync::mpsc::Receiver<()>,
    operation: DeliveryOperation<R>,
}

/// One typed result returned from the provider worker.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DeliveryExecutorResult<T, R> {
    pub(crate) input: T,
    pub(crate) output: R,
}

/// Nonblocking state of the bounded provider result channel.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DeliveryExecutorPoll<T, R> {
    Pending,
    Ready(DeliveryExecutorResult<T, R>),
    Disconnected,
}

/// Reservation failure that returns ownership before provider IO can begin.
pub(crate) struct DeliveryExecutorFull<T>(T);

impl<T> DeliveryExecutorFull<T> {
    pub(crate) fn into_input(self) -> T {
        self.0
    }
}

impl<T> std::fmt::Debug for DeliveryExecutorFull<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DeliveryExecutorFull(<redacted>)")
    }
}

/// A bounded single-worker executor whose reservation precedes durable IO-start publication.
pub(crate) struct BoundedDeliveryExecutor<T, R> {
    sender: Option<std::sync::mpsc::SyncSender<ReservedDelivery<T, R>>>,
    results: std::sync::mpsc::Receiver<DeliveryExecutorResult<T, R>>,
    _worker: std::thread::JoinHandle<()>,
}

impl<T, R> BoundedDeliveryExecutor<T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    pub(crate) fn new(capacity: usize, name: &str) -> std::io::Result<Self> {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<ReservedDelivery<T, R>>(capacity);
        let (result_sender, results) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                while let Ok(work) = receiver.recv() {
                    if work.start.recv().is_err() {
                        continue;
                    }
                    let output = (work.operation)();
                    if result_sender
                        .send(DeliveryExecutorResult {
                            input: work.input,
                            output,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            results,
            _worker: worker,
        })
    }

    pub(crate) fn reserve(
        &self,
        input: T,
        operation: impl FnOnce() -> R + Send + 'static,
    ) -> Result<DeliveryExecutorPermit, DeliveryExecutorFull<T>> {
        let (start, wait) = std::sync::mpsc::sync_channel(0);
        let work = ReservedDelivery {
            input,
            start: wait,
            operation: Box::new(operation),
        };
        let Some(sender) = &self.sender else {
            return Err(DeliveryExecutorFull(work.input));
        };
        match sender.try_send(work) {
            Ok(()) => Ok(DeliveryExecutorPermit { start: Some(start) }),
            Err(
                std::sync::mpsc::TrySendError::Full(work)
                | std::sync::mpsc::TrySendError::Disconnected(work),
            ) => Err(DeliveryExecutorFull(work.input)),
        }
    }

    pub(crate) fn poll(&self) -> DeliveryExecutorPoll<T, R> {
        match self.results.try_recv() {
            Ok(result) => DeliveryExecutorPoll::Ready(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => DeliveryExecutorPoll::Pending,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => DeliveryExecutorPoll::Disconnected,
        }
    }
}

impl<T, R> Drop for BoundedDeliveryExecutor<T, R> {
    fn drop(&mut self) {
        self.sender.take();
    }
}

/// A queue slot whose provider operation cannot run until `start` is called.
pub(crate) struct DeliveryExecutorPermit {
    start: Option<std::sync::mpsc::SyncSender<()>>,
}

impl DeliveryExecutorPermit {
    pub(crate) fn start(mut self) -> Result<(), std::sync::mpsc::SendError<()>> {
        self.start
            .take()
            .expect("delivery permit start sender exists")
            .send(())
    }
}

impl std::fmt::Debug for DeliveryExecutorPermit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DeliveryExecutorPermit(<redacted>)")
    }
}
