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
use crate::state::{Db, SessionScope};
use crate::tasks::cli::Cli;
use crate::tasks::selector::Selector;
use crate::tasks::task::AssignmentContext;
use crate::tasks::view::{View, build_view};
use crate::tui::{App, BrainTab, Panel, PanelSide, ZshFunctionRunner, handle_brain_key};
use crate::workspace::{
    CommandContext, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
};

use super::launch::register_fresh_before_launch;

mod fixtures;
mod launch;
mod lifecycle;
mod receiver;
mod triage;

use fixtures::*;
