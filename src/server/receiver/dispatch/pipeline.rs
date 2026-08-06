use super::*;

pub(super) struct SharedReceiverPipeline<'a> {
    pub(super) route: Option<crate::server::workspace_route::ResolvedWorkspaceRoute>,
    pub(super) request: &'a mut crate::server::http::Request,
    pub(super) control: &'a std::sync::Mutex<crate::server::control::ControlServer>,
    pub(super) channel: crate::server::receiver::Channel,
    pub(super) handoff_deadline: Option<crate::server::http::deadline::HandoffDeadline>,
    pub(super) admission:
        Option<std::sync::Arc<crate::server::receiver::admission::ReceiverAdmission>>,
    #[cfg(test)]
    pub(super) admission_clock:
        Option<std::sync::Arc<dyn Fn() -> std::time::Instant + Send + Sync>>,
    #[cfg(test)]
    pub(super) after_final_intent_reload: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    pub(super) after_combined_commit: Option<CombinedCommitProbe>,
    #[cfg(test)]
    pub(super) before_final_admission: Option<Box<dyn Fn() + Send + Sync>>,
}

pub(super) struct ResolvedActor {
    actor: crate::actor::ActorContext,
    response_email: Option<String>,
    allowed_response_recipients: Vec<String>,
}

impl DispatchPipeline for SharedReceiverPipeline<'_> {
    type Workspace = crate::server::workspace_route::ResolvedWorkspaceRoute;
    type ProviderConfig = crate::server::receiver::http::ProviderConfig;
    type Authenticated = crate::server::receiver::http::AuthenticatedInbound;
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
        crate::server::receiver::http::ProviderConfig::load(workspace).map_err(|error| {
            DispatchHttpError {
                status: 503,
                unavailable: true,
                message: error.to_string(),
            }
            .into()
        })
    }

    fn verify_signature(&mut self, config: &Self::ProviderConfig) -> Result<Self::Authenticated> {
        crate::server::receiver::http::authenticate(self.request, config, self.channel).map_err(
            |error| {
                DispatchHttpError {
                    status: error.status(),
                    unavailable: error.unavailable(),
                    message: error.to_string(),
                }
                .into()
            },
        )
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
            crate::server::receiver::Channel::Sms => crate::actor::RequestIdentity::Sms {
                from: &authenticated.sender,
            },
            crate::server::receiver::Channel::Email => crate::actor::RequestIdentity::Email {
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
            email_reply: authenticated.email_reply.clone(),
        })
    }

    fn forward(&mut self, workspace: &Self::Workspace, job: &Self::Job) -> Result<()> {
        let handoff_deadline = self
            .handoff_deadline
            .as_ref()
            .context("receiver handoff deadline was not prepared")?;
        let admission = self
            .admission
            .as_ref()
            .context("receiver admission was not prepared")?
            .clone();
        let control = self.control;
        #[cfg(test)]
        let authorize_clock = self.admission_clock.clone();
        #[cfg(test)]
        let commit_clock = self.admission_clock.clone();
        #[cfg(test)]
        let authorize_intent_hook = self.after_final_intent_reload.clone();
        #[cfg(test)]
        let commit_intent_hook = self.after_final_intent_reload.clone();
        #[cfg(test)]
        let commit_probe = self.after_combined_commit.clone();
        let authorize = || {
            #[cfg(test)]
            let clock = || {
                authorize_clock
                    .as_ref()
                    .map_or_else(std::time::Instant::now, |clock| clock())
            };
            #[cfg(not(test))]
            let clock = std::time::Instant::now;
            final_admission(
                control,
                workspace,
                &clock,
                #[cfg(test)]
                authorize_intent_hook.as_deref(),
            )?;
            admission.authorize()?;
            #[cfg(test)]
            if let Some(hook) = &self.before_final_admission {
                hook();
            }
            Ok(())
        };
        let commit = || {
            #[cfg(test)]
            let clock = || {
                commit_clock
                    .as_ref()
                    .map_or_else(std::time::Instant::now, |clock| clock())
            };
            #[cfg(not(test))]
            let clock = std::time::Instant::now;
            commit_admission(
                control,
                workspace,
                &admission,
                &clock,
                #[cfg(test)]
                commit_intent_hook.as_deref(),
                #[cfg(test)]
                commit_probe.as_deref(),
            )
        };
        let result = job.provider_id.as_ref().map_or_else(
            || {
                forward_job_until_with_admission(
                    &workspace.lease().job_socket,
                    job,
                    handoff_deadline,
                    authorize,
                    commit,
                )
            },
            |provider_id| {
                let key = (job.workspace_id, job.channel, provider_id.clone());
                forward_provider_delivery(&DELIVERIES, &key, || {
                    forward_job_until_with_admission(
                        &workspace.lease().job_socket,
                        job,
                        handoff_deadline,
                        authorize,
                        commit,
                    )
                })
            },
        );
        admission.complete();
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
        workspace.revalidate_receiver_intent().map_err(|error| {
            anyhow::Error::new(DispatchHttpError {
                status: 503,
                unavailable: true,
                message: error.to_string(),
            })
        })?;
        let admission = self
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .begin_receiver_admission(workspace, std::time::Instant::now())
            .map_err(|error| {
                anyhow::Error::new(DispatchHttpError {
                    status: 503,
                    unavailable: true,
                    message: error.to_string(),
                })
            })?;
        self.admission = Some(admission);
        self.handoff_deadline = Some(handoff_deadline);
        Ok(())
    }
}
