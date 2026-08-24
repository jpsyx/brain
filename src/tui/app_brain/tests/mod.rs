use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::NaiveDate;
use clap::Parser;
use crossterm::event::KeyCode;

use crate::agent::{
    AgentController, AgentError, AgentFrontend, AgentSession, AgentTransport, CompletionStrategy,
    HookMetadata, InputSequence, LaunchRequest, LaunchSpec, SessionStore,
};
use crate::config::Config;
use crate::pty_pane::PtyPane;
use crate::server::receiver::{Channel, InboundJob};
use crate::session;
use crate::session::AgentKind;
use crate::state::PanelSide;
use crate::state::{Db, SessionScope};
use crate::tasks::cli::Cli;
use crate::tasks::selector::Selector;
use crate::tasks::task::AssignmentContext;
use crate::tasks::view::{View, build_view};
use crate::tui::App;
use crate::tui::handlers::handle_brain_key;
use crate::tui::model::{BrainTab, Panel};
use crate::tui::shell::ZshFunctionRunner;
use crate::workspace::{
    CommandContext, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
};

fn receiver_job(
    app: &App,
    actor: crate::actor::ActorContext,
    channel: Channel,
    prompt: &str,
) -> InboundJob {
    InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: app.context.workspace().id(),
        actor,
        channel,
        prompt: prompt.to_owned(),
        authenticated_sender: "+15551234567".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 1,
        provider_id: Some("provider-message-1".to_owned()),
        thread_participants: vec!["+15551234567".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    }
}

use super::launch::register_fresh_before_launch;

mod fixtures;
mod input;
mod launch;
mod lifecycle;
mod opencode_launch;
mod opencode_receiver;
mod overlay_draw;
mod receiver;
mod receiver_durable_control_race;
mod receiver_durable_control_sync;
mod receiver_durable_controls;
mod receiver_durable_launch;
mod receiver_durable_lifecycle;
mod receiver_durable_support;
mod receiver_sync;
mod receiver_tab;
mod skill_session;
mod triage_overlay;

use fixtures::*;
