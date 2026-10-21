use super::*;
use crate::tasks::{removal::model::Options, store_lock::TaskStoreOwner};
use crate::tui::task_removal::worker::{NativeService, RemovalService, Request, Response};
use std::time::{Duration, Instant};

fn receive(service: &mut NativeService) -> (u64, Response) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = service.poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "native worker timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn native_task_removal_preview_and_apply_wait_for_ownership_off_the_event_loop() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let app = test_app(&temporary, &cli, AgentKind::Claude);
    let context = app.context.command();
    std::fs::write(
        context.workspace.root().join("tasks/tasks.csv"),
        "task_id,task_name\nT1,Remove me\n",
    )
    .unwrap();
    let mut service = NativeService::default();
    let owner = TaskStoreOwner::acquire(&context.workspace).unwrap();
    service.start(
        context.clone(),
        1,
        Request::Preview {
            targets: vec!["T1".into()],
            options: Options::default(),
            today: chrono::Local::now().date_naive(),
        },
    );
    assert!(service.poll().is_none());
    drop(owner);
    let (generation, result) = receive(&mut service);
    assert_eq!(generation, 1);
    let Response::Preview(Ok(plan)) = result else {
        panic!("expected native preview");
    };
    let owner = TaskStoreOwner::acquire(&context.workspace).unwrap();
    service.start(
        context.clone(),
        2,
        Request::Apply {
            expected: plan.fingerprint.clone(),
            plan: Box::new(plan),
            today: chrono::Local::now().date_naive(),
        },
    );
    assert!(service.poll().is_none());
    assert!(
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .contains("T1")
    );
    drop(owner);
    let (generation, result) = receive(&mut service);
    assert_eq!(generation, 2);
    let Response::Receipt(Ok(receipt)) = result else {
        panic!("expected committed receipt");
    };
    assert!(receipt.committed);
    service.start(
        context.clone(),
        3,
        Request::Status(receipt.operation_id.clone()),
    );
    assert!(
        matches!(receive(&mut service), (3, Response::Operation(Ok(operation))) if operation.receipt == Some(receipt.clone()))
    );
    service.start(
        context.clone(),
        4,
        Request::Resume(receipt.operation_id.clone()),
    );
    assert!(
        matches!(receive(&mut service), (4, Response::Receipt(Ok(resumed))) if resumed == receipt)
    );
    service.start(context.clone(), 5, Request::Abandon(receipt.operation_id));
    assert!(
        matches!(receive(&mut service), (5, Response::Operation(Err(problem))) if problem.code == "already_committed")
    );
}

#[path = "../../../../tests/support/task_removal_canonical.rs"]
mod canonical;

#[test]
fn canonical_middle_removal_tui_matches_cli_after_images_without_agent_prompt() {
    use crate::tui::{overlay::Overlay, task_removal::RemovalScreen};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let context = app.context.command().clone();
    let date = chrono::Local::now().date_naive().to_string();
    let directory = temporary.path().join("agendas");
    let agenda = directory.join(format!("{date}.md"));
    crate::env::set(&context, "agenda_markdown_dir", directory.to_str().unwrap()).unwrap();
    crate::settings::set(
        &context.workspace,
        "agenda_dir",
        directory.to_str().unwrap(),
    )
    .unwrap();
    crate::env::set(&context, "markdown_to_pdf_path", "/usr/bin/true").unwrap();
    canonical::seed(context.workspace.root(), &agenda);
    let before = canonical::images(context.workspace.root(), &agenda);
    let recording = TransportRecording::default();
    app.brain.replace_brain_transport(recording.transport());
    app.run_remove("T2");
    // The default service performs actual native IO; the modal owns consent.
    for choose in [false, true] {
        if choose {
            for code in [KeyCode::Down, KeyCode::Enter] {
                crate::tui::task_removal::handle_key(
                    &mut app,
                    &KeyEvent::new(code, KeyModifiers::NONE),
                );
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.tick_removal();
            if matches!(app.overlay, Some(Overlay::TaskRemoval(ref state)) if matches!(state.screen, RemovalScreen::Preview(_)))
            {
                break;
            }
            assert!(Instant::now() < deadline, "preview timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(canonical::images(context.workspace.root(), &agenda), before);
    }
    assert!(
        matches!(app.overlay, Some(Overlay::TaskRemoval(ref state)) if matches!(&state.screen, RemovalScreen::Preview(plan) if plan.blockers.is_empty() && plan.options.dependents == crate::tasks::removal::model::Dependents::Rewire))
    );
    for code in [KeyCode::Down, KeyCode::Enter] {
        crate::tui::task_removal::handle_key(&mut app, &KeyEvent::new(code, KeyModifiers::NONE));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.tick_removal();
        if matches!(app.overlay, Some(Overlay::TaskRemoval(ref state)) if matches!(state.screen, RemovalScreen::Finished(_)))
        {
            break;
        }
        assert!(Instant::now() < deadline, "apply timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    canonical::assert_after(context.workspace.root(), &agenda, &date);
    assert!(recording.launch_specs().is_empty());
    assert!(recording.inputs().is_empty());
}
