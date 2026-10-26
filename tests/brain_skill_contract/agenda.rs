use super::{catalog, markdown};

#[test]
fn task_only_requests_do_not_load_agenda_planning() {
    let docs = catalog::documents(false);
    let todo = &docs["todo/SKILL.md"];
    for request in ["/todo today", "/todo what", "/todo next", "task counts"] {
        let row = todo
            .lines()
            .find(|line| line.starts_with('|') && line.contains(request))
            .unwrap_or_else(|| panic!("missing task-only route: {request}"));
        assert!(row.contains("No agenda references or artifacts"), "{row}");
    }
    for alias in ["/todo agenda", "/todo plan-day"] {
        let row = todo
            .lines()
            .find(|line| line.starts_with('|') && line.contains(alias))
            .unwrap();
        assert!(row.contains("/agenda"));
    }
    assert!(!todo.contains("brain:ext todo:anchors"));
    assert!(!todo.contains("/tmp/"));
}

#[test]
fn agenda_modes_preserve_distinct_first_build_update_sync_and_regeneration() {
    let docs = catalog::documents(false);
    let agenda = docs.get("agenda/SKILL.md").expect("canonical agenda skill");
    for (request, behavior) in [
        ("First build", "collaborative draft"),
        ("Routine update", "settled choices"),
        ("Native mutation", "Rust"),
        ("Explicit regeneration", "full planning"),
    ] {
        let row = agenda
            .lines()
            .find(|line| line.starts_with('|') && line.contains(request))
            .unwrap();
        assert!(row.contains(behavior), "{row}");
    }
    let artifacts = &docs["agenda/references/artifacts.md"];
    for required in [
        "brain env get agenda_markdown_dir",
        "brain config get agenda_dir",
        "brain tasks agenda-pdf",
        "--font-shrink",
        "Completed today",
        "only if it already exists",
    ] {
        assert!(artifacts.contains(required), "missing {required}");
    }
    let planning = &docs["agenda/references/planning.md"];
    for required in [
        "date +%F",
        "ideal_time",
        "ceil(R / days_remaining)",
        "free time",
        "start_date",
        "stale-waiting",
    ] {
        assert!(planning.contains(required), "missing {required}");
    }
    for (path, text) in docs.iter().filter(|(path, _)| path.starts_with("agenda/")) {
        for forbidden in [
            "/tmp/",
            "H35",
            "H8",
            "Pray",
            "9:00 PM",
            "at least 2 code",
            "AskUserQuestion",
        ] {
            assert!(!text.contains(forbidden), "{path}: {forbidden}");
        }
    }
    assert!(
        markdown::links(agenda)
            .iter()
            .any(|link| link.ends_with("planning.md"))
    );
}

#[test]
fn triage_keeps_hooks_and_uses_native_managed_completion_before_signal() {
    let docs = catalog::documents(false);
    let triage = &docs["triage/SKILL.md"];
    for hook in [
        "daily-open",
        "daily-subagents",
        "daily-linear",
        "daily-merge",
        "daily-required-outputs",
        "weekly-inboxes",
        "weekly-linear",
    ] {
        assert_eq!(
            triage
                .matches(&format!("brain:ext triage:{hook} -->"))
                .count(),
            1
        );
    }
    assert!(triage.contains("brain habits complete-managed-triage daily"));
    assert!(triage.contains("BRAIN_SESSION_TOKEN"));
    assert!(triage.contains("require"));
    assert!(!triage.contains("helper doesn't sync"));
    assert!(!docs["todo/SKILL.md"].contains("brain habits skip \"Morning Triage\""));
    assert!(!triage.contains("AskUserQuestion"));
    let bulk = triage.find("## Step 3").unwrap();
    assert!(triage[..bulk].contains("hard_deadline=true"));
}

#[test]
fn task_references_use_current_detector_and_no_penalty_defer_contracts() {
    let docs = catalog::documents(false);
    let schema = &docs["todo/references/schema.md"];
    assert!(!schema.contains("due_date > today + 14"));
    assert!(!schema.contains("Increments on every defer"));
    assert!(schema.contains("brain tasks chronic"));
    assert!(schema.contains("today + 3"));
}

#[test]
fn capture_only_requests_do_not_enter_or_complete_weekly_triage() {
    let docs = catalog::documents(false);
    let triage = &docs["triage/SKILL.md"];
    let scope = triage
        .split("## Capture-only requests")
        .nth(1)
        .expect("capture-only routing must be explicit")
        .split("## Triage never touches habits")
        .next()
        .unwrap();
    assert!(scope.contains("/process-capture"));
    assert!(scope.contains("Do not run weekly review"));
    assert!(scope.contains("complete the weekly habit"));
    assert!(scope.contains("Explicit weekly review"));
    assert!(!triage.contains("\"process my capture\""));
}
