use super::{add_task_prompt, reassign_task_prompt, start_task_prompt};
use std::path::Path;

#[test]
fn start_task_prompt_interpolates_the_configured_root() {
    let p = start_task_prompt("T7", Path::new("/srv/brain"));
    assert!(p.contains("T7"));
    assert!(p.contains("/srv/brain/tasks/tasks.csv"));
    assert!(p.contains("/srv/brain/projects"));
}

#[test]
fn start_task_prompt_never_hardcodes_tilde_brain() {
    let p = start_task_prompt("T1", Path::new("/custom/root"));
    assert!(!p.contains("~/brain"));
}

#[test]
fn add_task_prompt_defaults_assignment_to_the_current_actor() {
    let prompt = add_task_prompt("wife");

    assert!(prompt.contains("/todo add"));
    assert!(prompt.contains("assigned_to=wife"));
    assert!(prompt.contains("unless I explicitly choose another workspace member"));
}

#[test]
fn reassign_task_prompt_targets_the_selected_task() {
    let prompt = reassign_task_prompt("T7");

    assert!(prompt.contains("/todo assign T7"));
    assert!(prompt.contains("workspace member"));
}

#[test]
fn agenda_prompt_delegates_to_canonical_skill_and_configured_destinations() {
    let prompt = super::generate_agenda_prompt();
    assert!(prompt.contains("/agenda"));
    assert!(prompt.contains("brain env get agenda_markdown_dir"));
    assert!(prompt.contains("brain config get agenda_dir"));
    assert!(!prompt.contains("/tmp"));
    assert!(!prompt.contains("/todo"));
}
