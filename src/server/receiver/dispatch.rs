use anyhow::{Context, Result};

use super::InboundJob;

#[path = "dispatch/deliveries.rs"]
mod deliveries;
#[path = "dispatch/final_authority.rs"]
mod final_authority;
#[path = "dispatch/pipeline.rs"]
mod pipeline;
use deliveries::{DELIVERIES, forward_provider_delivery};
pub(in crate::server) use deliveries::{
    provider_delivery_was_discarded, remember_verified_unavailable_email,
};
use final_authority::{commit_admission, final_admission};
use pipeline::SharedReceiverPipeline;

pub(crate) const JOB_FRAME_LIMIT: usize = 1024 * 1024;

#[cfg(test)]
type CombinedCommitProbe =
    std::sync::Arc<dyn Fn(&super::admission::ReceiverAdmission) + Send + Sync>;

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
pub(crate) struct DispatchHttpError {
    status: u16,
    unavailable: bool,
    message: String,
}

impl std::fmt::Debug for DispatchHttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DispatchHttpError")
            .field("status", &self.status)
            .field("unavailable", &self.unavailable)
            .field("message", &"<redacted>")
            .finish()
    }
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
    body: &[u8],
    control: &std::sync::Mutex<crate::server::control::ControlServer>,
    channel: super::Channel,
) -> Result<InboundJob, DispatchHttpError> {
    let mut pipeline = SharedReceiverPipeline {
        route: Some(route),
        request,
        body,
        control,
        channel,
        handoff_deadline: None,
        admission: None,
        #[cfg(test)]
        admission_clock: None,
        #[cfg(test)]
        after_final_intent_reload: None,
        #[cfg(test)]
        after_combined_commit: None,
        #[cfg(test)]
        before_final_admission: None,
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

#[cfg(test)]
mod tests;
