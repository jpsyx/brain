#[path = "analysis/collect.rs"]
mod collect;
#[path = "analysis/facts.rs"]
mod facts;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use collect::collect_program;
pub(super) use facts::TypeFact;

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
    pub(super) global_consumer_violations: Vec<String>,
}

#[derive(Default)]
pub(super) struct Program {
    pub(super) functions: HashMap<String, FunctionNode>,
    pub(super) receiver_tick_calls: usize,
}

impl Program {
    pub(super) fn merge_function(&mut self, mut node: FunctionNode) {
        let Some(existing) = self.functions.get_mut(&node.id) else {
            self.functions.insert(node.id.clone(), node);
            return;
        };
        existing.receiver_owned |= node.receiver_owned;
        existing.calls.append(&mut node.calls);
        existing.violations.append(&mut node.violations);
        existing
            .global_consumer_violations
            .append(&mut node.global_consumer_violations);
    }
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
    violations.extend(program.functions.values().flat_map(|function| {
        function
            .global_consumer_violations
            .iter()
            .map(|violation| format!("{}: {violation}", function.id))
    }));
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
    method == "tick_receiver" && owner.any_variant(|variant| variant.app)
}

pub(super) fn classify_operation(owner: &TypeFact, method: &str) -> Option<&'static str> {
    owner
        .variants()
        .find_map(|variant| classify_single_operation(variant, method))
}

pub(super) fn classify_method_operation(owner: &TypeFact, method: &str) -> Option<&'static str> {
    if matches!(
        method,
        "park_timeout" | "wait_timeout" | "wait_timeout_while"
    ) && !owner.any_variant(|variant| variant.condition_variable)
    {
        Some("blocking activity wait")
    } else {
        classify_operation(owner, method)
    }
}

fn classify_single_operation(owner: &TypeFact, method: &str) -> Option<&'static str> {
    if owner.unresolved_glob {
        return Some("unresolved glob-owned type operation");
    }
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
            "configured_with_command"
            | "ensure_available"
            | "kind"
            | "launch"
            | "new"
            | "resume_candidate_exists"
            | "shutdown" => None,
            _ => Some("unclassified AgentController operation"),
        };
    }
    if owner.app || owner.brain_panel {
        return match method {
            "open_or_focus_brain" => Some("interactive main-panel focus"),
            "take_main" | "install_main" | "main_controller" | "main_controller_mut" => {
                Some("interactive main-panel controller access")
            }
            "active_brain_controller" | "active_brain_controller_mut" => {
                Some("interactive selected-panel controller access")
            }
            "focus_brain" | "select_brain_tab" | "select_brain_tab_slot" | "cycle_brain_tab" => {
                Some("interactive selected-panel takeover")
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
    if is_inbound_channel_consumer(owner, method) {
        return Some("in-memory receiver channel consume");
    }
    if is_inbound_queue_consumer(owner, method) {
        return Some("in-memory receiver queue consume");
    }
    None
}

pub(super) fn classify_function_call(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("std::thread::sleep" | "tokio::time::sleep" | "std::thread::park_timeout") => {
            Some("blocking activity wait")
        }
        _ => None,
    }
}

pub(super) fn is_global_inbound_consumer(owner: &TypeFact, method: &str) -> bool {
    is_inbound_channel_consumer(owner, method) || is_inbound_queue_consumer(owner, method)
}

fn is_inbound_channel_consumer(owner: &TypeFact, method: &str) -> bool {
    owner.any_variant(|variant| {
        variant.channel_receiver
            && variant.inbound_job
            && matches!(
                method,
                "recv" | "try_recv" | "recv_timeout" | "iter" | "try_iter"
            )
    })
}

fn is_inbound_queue_consumer(owner: &TypeFact, method: &str) -> bool {
    owner.any_variant(|variant| {
        variant.memory_queue
            && variant.inbound_job
            && matches!(method, "pop_front" | "pop_back" | "remove" | "drain")
    })
}

pub(super) fn classify_into_iteration(owner: &TypeFact) -> Option<&'static str> {
    owner.variants().find_map(|variant| {
        if variant.channel_receiver && variant.inbound_job {
            Some("in-memory receiver channel consume")
        } else if variant.memory_queue && variant.inbound_job && !variant.borrowed {
            Some("in-memory receiver queue consume")
        } else {
            None
        }
    })
}

pub(super) fn is_into_iterator_dispatch(owner: &TypeFact, method: &str) -> bool {
    method == "into_iter"
        && owner.any_variant(|variant| {
            variant.channel_receiver
                || variant.memory_queue
                || matches!(
                    variant.canonical.as_deref(),
                    Some("std::iter::IntoIterator" | "core::iter::IntoIterator")
                )
        })
}
