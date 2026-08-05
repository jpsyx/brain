use std::path::Path;

use anyhow::{Context, Result};

use super::InboundJob;

pub(crate) const JOB_FRAME_LIMIT: usize = 1024 * 1024;

/// Ordered decision boundary for one inbound receiver request.
pub trait DispatchPipeline {
    type Workspace;
    type ProviderConfig;
    type Authenticated;
    type Actor;
    type Job;

    fn resolve_workspace(&mut self) -> Result<Self::Workspace>;
    fn load_provider_config(&mut self, workspace: &Self::Workspace)
    -> Result<Self::ProviderConfig>;
    fn verify_signature(&mut self, config: &Self::ProviderConfig) -> Result<Self::Authenticated>;
    fn resolve_actor(
        &mut self,
        workspace: &Self::Workspace,
        authenticated: &Self::Authenticated,
    ) -> Result<Self::Actor>;
    fn build_job(
        &mut self,
        workspace: &Self::Workspace,
        actor: &Self::Actor,
        authenticated: &Self::Authenticated,
    ) -> Result<Self::Job>;
    fn revalidate_authority(&mut self, workspace: &Self::Workspace, job: &Self::Job) -> Result<()>;
    fn forward(&mut self, workspace: &Self::Workspace, job: &Self::Job) -> Result<()>;
}

/// Execute the receiver decisions in their security-sensitive order.
///
/// # Errors
///
/// Stops at the first rejected decision. Later workspace-specific stages are
/// never invoked after an earlier failure.
pub fn execute_pipeline<P: DispatchPipeline>(pipeline: &mut P) -> Result<P::Job> {
    let workspace = pipeline.resolve_workspace()?;
    let config = pipeline.load_provider_config(&workspace)?;
    let authenticated = pipeline.verify_signature(&config)?;
    let actor = pipeline.resolve_actor(&workspace, &authenticated)?;
    let job = pipeline.build_job(&workspace, &actor, &authenticated)?;
    pipeline.revalidate_authority(&workspace, &job)?;
    pipeline.forward(&workspace, &job)?;
    Ok(job)
}

/// Failure returned to the shared HTTP boundary.
#[derive(Debug)]
pub(crate) struct DispatchHttpError {
    status: u16,
    unavailable: bool,
    message: String,
}

impl DispatchHttpError {
    pub(crate) const fn status(&self) -> u16 {
        self.status
    }

    pub(crate) const fn unavailable(&self) -> bool {
        self.unavailable
    }
}

impl std::fmt::Display for DispatchHttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DispatchHttpError {}

pub(in crate::server) fn dispatch_http(
    route: crate::server::workspace_route::ResolvedWorkspaceRoute,
    request: &mut crate::server::http::Request,
    control: &std::sync::Mutex<crate::server::control::ControlServer>,
    channel: super::Channel,
) -> Result<InboundJob, DispatchHttpError> {
    let mut pipeline = SharedReceiverPipeline {
        route: Some(route),
        request,
        control,
        channel,
        handoff_deadline: None,
    };
    execute_pipeline(&mut pipeline).map_err(|error| {
        error
            .downcast::<DispatchHttpError>()
            .unwrap_or_else(|error| DispatchHttpError {
                status: 500,
                unavailable: false,
                message: error.to_string(),
            })
    })
}

struct SharedReceiverPipeline<'a> {
    route: Option<crate::server::workspace_route::ResolvedWorkspaceRoute>,
    request: &'a mut crate::server::http::Request,
    control: &'a std::sync::Mutex<crate::server::control::ControlServer>,
    channel: super::Channel,
    handoff_deadline: Option<crate::server::http::deadline::HandoffDeadline>,
}

struct ResolvedActor {
    actor: crate::actor::ActorContext,
    response_email: Option<String>,
    allowed_response_recipients: Vec<String>,
}

impl DispatchPipeline for SharedReceiverPipeline<'_> {
    type Workspace = crate::server::workspace_route::ResolvedWorkspaceRoute;
    type ProviderConfig = super::http::ProviderConfig;
    type Authenticated = super::http::AuthenticatedInbound;
    type Actor = ResolvedActor;
    type Job = InboundJob;

    fn resolve_workspace(&mut self) -> Result<Self::Workspace> {
        self.route
            .take()
            .context("workspace route was already consumed")
    }

    fn load_provider_config(
        &mut self,
        workspace: &Self::Workspace,
    ) -> Result<Self::ProviderConfig> {
        super::http::ProviderConfig::load(workspace).map_err(|error| {
            DispatchHttpError {
                status: 503,
                unavailable: true,
                message: error.to_string(),
            }
            .into()
        })
    }

    fn verify_signature(&mut self, config: &Self::ProviderConfig) -> Result<Self::Authenticated> {
        super::http::authenticate(self.request, config, self.channel).map_err(|error| {
            DispatchHttpError {
                status: error.status(),
                unavailable: error.unavailable(),
                message: error.to_string(),
            }
            .into()
        })
    }

    fn resolve_actor(
        &mut self,
        workspace: &Self::Workspace,
        authenticated: &Self::Authenticated,
    ) -> Result<Self::Actor> {
        let users = crate::users::UsersStore::load(workspace.context()).map_err(|error| {
            anyhow::Error::new(DispatchHttpError {
                status: 503,
                unavailable: true,
                message: format!("workspace users are unavailable: {error}"),
            })
        })?;
        let local_user = crate::users::UserId::parse(workspace.context().local_user_id())?;
        let identity = match authenticated.channel {
            super::Channel::Sms => crate::actor::RequestIdentity::Sms {
                from: &authenticated.sender,
            },
            super::Channel::Email => crate::actor::RequestIdentity::Email {
                from: &authenticated.sender,
            },
        };
        let actor = crate::server::security::resolve_authenticated_actor(
            true,
            &local_user,
            identity,
            &users,
        )
        .map_err(|_| {
            anyhow::Error::new(DispatchHttpError {
                status: 403,
                unavailable: false,
                message: "authenticated sender is not allowed in this workspace".to_owned(),
            })
        })?;
        let response_email = users
            .user(actor.user_id())
            .and_then(|user| user.response_email.clone());
        let allowed_response_recipients = crate::server::delivery::actor_thread_recipients(
            &authenticated.participants,
            &users,
            &actor,
            &authenticated.receiving_address,
        );
        Ok(ResolvedActor {
            actor,
            response_email,
            allowed_response_recipients,
        })
    }

    fn build_job(
        &mut self,
        workspace: &Self::Workspace,
        actor: &Self::Actor,
        authenticated: &Self::Authenticated,
    ) -> Result<Self::Job> {
        let received_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("system clock precedes Unix epoch")?
            .as_millis()
            .try_into()
            .context("inbound receipt time exceeds u64")?;
        Ok(InboundJob {
            job_id: uuid::Uuid::new_v4(),
            workspace_id: workspace.context().id(),
            actor: actor.actor.clone(),
            channel: authenticated.channel,
            authenticated_sender: authenticated.sender.clone(),
            prompt: authenticated.prompt.clone(),
            attachments: authenticated.attachments.clone(),
            received_at_unix_ms,
            provider_id: authenticated.provider_id.clone(),
            thread_participants: authenticated.participants.clone(),
            response_email: actor.response_email.clone(),
            allowed_response_recipients: actor.allowed_response_recipients.clone(),
        })
    }

    fn forward(&mut self, workspace: &Self::Workspace, job: &Self::Job) -> Result<()> {
        static DELIVERIES: std::sync::LazyLock<std::sync::Mutex<ProviderDeliveries>> =
            std::sync::LazyLock::new(|| std::sync::Mutex::new(ProviderDeliveries::default()));
        let handoff_deadline = self
            .handoff_deadline
            .as_ref()
            .context("receiver handoff deadline was not prepared")?;
        let result = job.provider_id.as_ref().map_or_else(
            || forward_job_until(&workspace.lease().job_socket, job, handoff_deadline),
            |provider_id| {
                let key = (job.workspace_id, job.channel, provider_id.clone());
                forward_provider_delivery(&DELIVERIES, &key, || {
                    forward_job_until(&workspace.lease().job_socket, job, handoff_deadline)
                })
            },
        );
        result.map_err(|error| {
            DispatchHttpError {
                status: 503,
                unavailable: true,
                message: format!("live workspace TUI did not accept the job: {error}"),
            }
            .into()
        })
    }

    fn revalidate_authority(
        &mut self,
        workspace: &Self::Workspace,
        _job: &Self::Job,
    ) -> Result<()> {
        let handoff_deadline = self.request.job_handoff_deadline().map_err(|error| {
            anyhow::Error::new(DispatchHttpError {
                status: 503,
                unavailable: true,
                message: format!("receiver deadline cannot cover enqueue and response: {error}"),
            })
        })?;
        self.control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revalidate_workspace_route(workspace, std::time::Instant::now())
            .map_err(|error| {
                anyhow::Error::new(DispatchHttpError {
                    status: 503,
                    unavailable: true,
                    message: error.to_string(),
                })
            })?;
        self.handoff_deadline = Some(handoff_deadline);
        Ok(())
    }
}

type ProviderKey = (crate::workspace::WorkspaceId, super::Channel, String);

fn forward_provider_delivery(
    deliveries: &std::sync::Mutex<ProviderDeliveries>,
    key: &ProviderKey,
    forward: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let reservation = deliveries
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .begin(key.clone());
    match reservation {
        ProviderReservation::Duplicate => return Ok(()),
        ProviderReservation::InFlight => {
            anyhow::bail!("provider delivery is already being accepted")
        }
        ProviderReservation::Started => {}
    }
    let result = forward();
    deliveries
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish(key, result.is_ok());
    result
}

#[derive(Default)]
struct ProviderDeliveries {
    pending: std::collections::HashSet<ProviderKey>,
    order: std::collections::VecDeque<ProviderKey>,
    accepted: std::collections::HashSet<ProviderKey>,
}

impl ProviderDeliveries {
    fn begin(&mut self, key: ProviderKey) -> ProviderReservation {
        if self.accepted.contains(&key) {
            return ProviderReservation::Duplicate;
        }
        if !self.pending.insert(key) {
            return ProviderReservation::InFlight;
        }
        ProviderReservation::Started
    }

    fn finish(&mut self, key: &ProviderKey, accepted: bool) {
        const RECENT_PROVIDER_IDS: usize = 1024;
        self.pending.remove(key);
        if !accepted || !self.accepted.insert(key.clone()) {
            return;
        }
        self.order.push_back(key.clone());
        while self.order.len() > RECENT_PROVIDER_IDS {
            if let Some(expired) = self.order.pop_front() {
                self.accepted.remove(&expired);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderReservation {
    Started,
    Duplicate,
    InFlight,
}

impl ProviderReservation {
    #[cfg(test)]
    const fn started(self) -> bool {
        matches!(self, Self::Started)
    }
}

/// Forward one bounded job frame to an already-live TUI and await enqueue.
///
/// # Errors
///
/// Returns an error when the live socket cannot be reached, the frame is too
/// large, or the receiving TUI does not acknowledge its in-memory enqueue.
pub fn forward_job(path: &Path, job: &InboundJob) -> Result<()> {
    let deadline = crate::server::http::deadline::HandoffDeadline::from_now(
        super::http::RECEIVER_JOB_HANDOFF_TIMEOUT,
    )?;
    forward_job_until(path, job, &deadline)
}

fn forward_job_until(
    path: &Path,
    job: &InboundJob,
    deadline: &crate::server::http::deadline::HandoffDeadline,
) -> Result<()> {
    let frame = serde_json::to_vec(job).context("serializing inbound job")?;
    anyhow::ensure!(
        frame.len() <= JOB_FRAME_LIMIT,
        "inbound job exceeds the socket frame limit"
    );
    super::transport::forward_serialized_until(path, &frame, deadline)
        .context("forwarding job to the live workspace TUI")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier, Mutex};

    use super::{ProviderDeliveries, ProviderKey, forward_provider_delivery};
    use crate::server::receiver::Channel;
    use crate::workspace::WorkspaceId;

    #[test]
    fn failed_handoff_releases_provider_id_for_one_later_success() {
        let deliveries = Mutex::new(ProviderDeliveries::default());
        let key = key(PERSONAL_ID, Channel::Sms, "provider-1");
        let mut attempts = 0;

        assert!(
            forward_provider_delivery(&deliveries, &key, || {
                attempts += 1;
                anyhow::bail!("socket unavailable")
            })
            .is_err()
        );
        forward_provider_delivery(&deliveries, &key, || {
            attempts += 1;
            Ok(())
        })
        .unwrap();
        forward_provider_delivery(&deliveries, &key, || {
            attempts += 1;
            Ok(())
        })
        .unwrap();

        assert_eq!(attempts, 2);
    }

    #[test]
    fn in_flight_duplicate_is_not_acknowledged_before_first_handoff_finishes() {
        let deliveries = Arc::new(Mutex::new(ProviderDeliveries::default()));
        let key = key(PERSONAL_ID, Channel::Email, "provider-2");
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_deliveries = Arc::clone(&deliveries);
        let worker_key = key.clone();
        let worker_entered = Arc::clone(&entered);
        let worker_release = Arc::clone(&release);
        let worker = std::thread::spawn(move || {
            forward_provider_delivery(&worker_deliveries, &worker_key, || {
                worker_entered.wait();
                worker_release.wait();
                Ok(())
            })
        });
        entered.wait();

        assert!(forward_provider_delivery(&deliveries, &key, || Ok(())).is_err());
        release.wait();
        worker.join().unwrap().unwrap();
        forward_provider_delivery(&deliveries, &key, || {
            panic!("accepted duplicate reached the job socket")
        })
        .unwrap();
    }

    #[test]
    fn retained_provider_ids_are_bounded_and_workspace_channel_scoped() {
        let mut deliveries = ProviderDeliveries::default();
        for index in 0..=1024 {
            let key = key(PERSONAL_ID, Channel::Sms, &format!("provider-{index}"));
            assert!(deliveries.begin(key.clone()).started());
            deliveries.finish(&key, true);
        }

        assert!(
            deliveries
                .begin(key(PERSONAL_ID, Channel::Sms, "provider-0"))
                .started()
        );
        assert!(
            deliveries
                .begin(key(FAMILY_ID, Channel::Sms, "provider-1024"))
                .started()
        );
        assert!(
            deliveries
                .begin(key(PERSONAL_ID, Channel::Email, "provider-1024"))
                .started()
        );
    }

    const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
    const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

    fn key(workspace: &str, channel: Channel, provider_id: &str) -> ProviderKey {
        (
            WorkspaceId::parse(workspace).unwrap(),
            channel,
            provider_id.to_owned(),
        )
    }
}
