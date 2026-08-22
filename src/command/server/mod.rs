//! Server, receiver, and habits command handlers.

mod habits;
mod killall;
mod lifecycle;
mod receiver;

pub use habits::run_habits;
pub use killall::killall;
pub use lifecycle::run_server;
pub(crate) use receiver::apply_startup_receiver_flag;
pub(crate) use receiver::read_receiver_status;
pub(crate) use receiver::receiver_enabled;
pub(crate) use receiver::refresh_agent_hooks;
pub use receiver::run_receiver;
pub(crate) use receiver::update_json_file as update_agent_hook_json;
pub(crate) use receiver::write_workspace_artifact as write_agent_workspace_artifact;
pub(crate) use receiver::{
    ReceiverActionOutcome, ReceiverIntentRefresher, apply_receiver_action_with,
};
