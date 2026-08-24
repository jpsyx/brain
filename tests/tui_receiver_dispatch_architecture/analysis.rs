#[path = "analysis/collect.rs"]
mod collect;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use collect::collect_program;

#[derive(Clone, Debug, Default)]
pub(super) struct TypeFact {
    pub(super) canonical: Option<String>,
    pub(super) inbound_job: bool,
    pub(super) agent_controller: bool,
    pub(super) app: bool,
    pub(super) brain_panel: bool,
    pub(super) unix_listener: bool,
    pub(super) unix_stream: bool,
    pub(super) channel_receiver: bool,
    pub(super) memory_queue: bool,
}

#[derive(Clone, Debug)]
pub(super) struct RawCall {
    pub(super) exact_target: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct FunctionNode {
    pub(super) id: String,
    pub(super) receiver_owned: bool,
    pub(super) calls: Vec<RawCall>,
    pub(super) violations: Vec<String>,
}

#[derive(Default)]
pub(super) struct Program {
    pub(super) functions: HashMap<String, FunctionNode>,
    pub(super) receiver_tick_calls: usize,
}

pub(super) fn receiver_violations(root: &Path) -> Vec<String> {
    let program = collect_program(root);
    let mut reachable = HashSet::new();
    let mut pending = program
        .functions
        .values()
        .filter(|function| function.receiver_owned)
        .map(|function| function.id.clone())
        .collect::<VecDeque<_>>();

    while let Some(id) = pending.pop_front() {
        if !reachable.insert(id.clone()) {
            continue;
        }
        let Some(function) = program.functions.get(&id) else {
            continue;
        };
        for call in &function.calls {
            if let Some(target) = resolve_call(call, &program)
                && !reachable.contains(&target)
            {
                pending.push_back(target);
            }
        }
    }

    let mut violations = reachable
        .into_iter()
        .filter_map(|id| program.functions.get(&id))
        .flat_map(|function| {
            function
                .violations
                .iter()
                .map(|violation| format!("{}: {violation}", function.id))
        })
        .collect::<Vec<_>>();
    violations.sort();
    violations.dedup();
    violations
}

pub(super) fn receiver_tick_call_count(root: &Path) -> usize {
    collect_program(root).receiver_tick_calls
}

fn resolve_call(call: &RawCall, program: &Program) -> Option<String> {
    if let Some(exact) = &call.exact_target
        && program.functions.contains_key(exact)
    {
        return Some(exact.clone());
    }
    None
}

pub(super) fn receiver_owned_module(module: &[String]) -> bool {
    module.iter().any(|segment| {
        segment == "receiver" || segment.starts_with("receiver_") || segment.ends_with("_receiver")
    })
}

pub(super) fn is_receiver_tick_call(owner: &TypeFact, method: &str) -> bool {
    owner.app && method == "tick_receiver"
}

pub(super) fn classify_operation(owner: &TypeFact, method: &str) -> Option<&'static str> {
    if owner.agent_controller {
        return match method {
            "type_text" => Some("interactive AgentController type_text"),
            "submit_now" => Some("interactive AgentController submit_now"),
            "queue_after_active_turn" => {
                Some("interactive AgentController queue_after_active_turn")
            }
            "start_new_session" => Some("interactive AgentController start_new_session"),
            "forward_terminal_input" => Some("interactive AgentController forward_terminal_input"),
            "snapshot" | "terminal_screen" => Some("interactive AgentController activity sample"),
            _ => None,
        };
    }
    if owner.app || owner.brain_panel {
        return match method {
            "open_or_focus_brain" => Some("interactive main-panel focus"),
            "take_main" | "install_main" | "main_controller" | "main_controller_mut" => {
                Some("interactive main-panel controller access")
            }
            _ => None,
        };
    }
    if owner.unix_listener && method == "accept" {
        return Some("UnixListener accept");
    }
    if owner.unix_stream && matches!(method, "read" | "read_exact" | "read_to_end" | "read_line") {
        return Some("Unix socket read");
    }
    if owner.channel_receiver
        && owner.inbound_job
        && matches!(method, "recv" | "try_recv" | "recv_timeout")
    {
        return Some("in-memory receiver channel consume");
    }
    if owner.memory_queue
        && owner.inbound_job
        && matches!(method, "pop_front" | "pop_back" | "remove" | "drain")
    {
        return Some("in-memory receiver queue consume");
    }
    None
}
