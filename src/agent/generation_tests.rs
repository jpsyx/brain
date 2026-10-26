use std::{fs, sync::Arc};

use crate::{
    access::{AccessMode, MachineCapabilityEnvironment, capability_plan},
    agent::{AgentSession, LaunchRequest, SessionPlan},
    config::Config,
    workspace::{WorkspaceContext, WorkspaceId, WorkspaceManifest, WorkspaceName},
};

fn workspace(home: &std::path::Path) -> Arc<WorkspaceContext> {
    let root = home.join("workspace");
    fs::create_dir(&root).unwrap();
    let id = WorkspaceId::new();
    WorkspaceManifest::new(id).write_new(&root).unwrap();
    fs::write(
        root.join(".config/config.json"),
        r#"{"skills_auto_sync":false}"#,
    )
    .unwrap();
    Arc::new(
        WorkspaceContext::new(
            home,
            id,
            WorkspaceName::parse("alpha").unwrap(),
            &root,
            "member",
            &root,
        )
        .unwrap(),
    )
}

fn extensions(workspace: &WorkspaceContext, generation: &str) {
    let dir = workspace.root().join(".config/extensions");
    fs::create_dir_all(&dir).unwrap();
    for (name, contents) in [
        (
            "workspace",
            format!("[workspace:context]\n{generation} workspace\n"),
        ),
        (
            "todo",
            format!("{generation} task context\n[todo:anchors]\n{generation} anchor\n"),
        ),
        ("custom", format!("[custom:rules]\n{generation} custom\n")),
    ] {
        fs::write(dir.join(format!("{name}.md")), contents).unwrap();
    }
}

fn request(workspace: &Arc<WorkspaceContext>) -> LaunchRequest {
    let custom = workspace.root().join("machine/custom");
    fs::create_dir_all(&custom).unwrap();
    fs::write(
        custom.join("SKILL.md"),
        "# custom\n<!-- brain:ext custom:rules -->\n",
    )
    .unwrap();
    let machine = MachineCapabilityEnvironment::from_value(
        workspace.id(),
        serde_json::json!({
            "skills": [{"name": "custom", "path": custom}]
        }),
    )
    .unwrap();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_skills: vec!["agenda".into(), "custom".into()],
        ..Config::default()
    };
    LaunchRequest::from_trusted_context(
        Arc::clone(workspace),
        crate::actor::test_actor("member"),
        SessionPlan::fresh(AgentSession::new("generation-test").unwrap()),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(capability_plan(&config, &machine).unwrap())
}

fn assert_generation(request: &LaunchRequest, expected: &str, excluded: &str) {
    let prepared = request.prepare_instructions().unwrap();
    let guidance = prepared.instructions().unwrap().workspace_guidance();
    assert!(
        guidance.contains(&format!("{expected} workspace")),
        "{guidance}"
    );
    assert!(!guidance.contains(excluded));
    let report = prepared
        .render_capabilities(prepared.access_policy().capability_plan().unwrap())
        .unwrap();
    for (path, marker) in [
        ("agenda/SKILL.md", "anchor"),
        ("agenda/references/legacy-task-context.md", "task context"),
        ("custom/SKILL.md", "custom"),
    ] {
        let body = fs::read_to_string(report.rendered_dir.join(path)).unwrap();
        assert!(
            body.contains(&format!("{expected} {marker}")),
            "{path}: {body}"
        );
        assert!(!body.contains(excluded), "{path}: {body}");
    }
}

#[test]
fn disabled_optional_sync_keeps_published_generation_for_bundled_and_machine_selection() {
    let home = tempfile::tempdir().unwrap();
    let workspace = workspace(home.path());
    extensions(&workspace, "OLD");
    crate::skills::install::sync_workspace(&workspace, &crate::skills::real_sources(&workspace))
        .unwrap();
    let request = request(&workspace);
    extensions(&workspace, "NEW");
    crate::skills::resync_skills(&workspace);
    assert_generation(&request, "OLD", "NEW");
    crate::skills::install::sync_workspace(&workspace, &crate::skills::real_sources(&workspace))
        .unwrap();
    assert_generation(&request, "NEW", "OLD");
}

#[test]
fn package_retains_exact_checked_extension_snapshot() {
    let home = tempfile::tempdir().unwrap();
    let workspace = workspace(home.path());
    extensions(&workspace, "OLD");
    crate::skills::install::sync_workspace(&workspace, &crate::skills::real_sources(&workspace))
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(
            workspace
                .paths()
                .cache_dir()
                .join("skills-install/managed.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        manifest["package"]["extension_snapshot"]["sources"].is_object(),
        "package must retain captured extension bytes"
    );
}

#[test]
fn launch_preparation_repairs_older_metadata_without_a_snapshot() {
    let home = tempfile::tempdir().unwrap();
    let workspace = workspace(home.path());
    extensions(&workspace, "OLD");
    crate::skills::install::sync_workspace(&workspace, &crate::skills::real_sources(&workspace))
        .unwrap();
    let path = workspace
        .paths()
        .cache_dir()
        .join("skills-install/managed.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["package"]
        .as_object_mut()
        .unwrap()
        .remove("extension_snapshot");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    extensions(&workspace, "NEW");
    let prepared = request(&workspace).prepare_instructions().unwrap();
    assert!(
        prepared
            .instructions()
            .unwrap()
            .workspace_guidance()
            .contains("NEW workspace"),
        "missing snapshot requires whole-package repair before launch"
    );
    let repaired: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(repaired["package"]["extension_snapshot"].is_object());
}

#[test]
fn launch_preparation_obeys_sync_then_install_ownership() {
    let home = tempfile::tempdir().unwrap();
    let workspace = workspace(home.path());
    crate::skills::install::sync_workspace(&workspace, &crate::skills::real_sources(&workspace))
        .unwrap();
    let request = request(&workspace);
    let identity =
        crate::skills::install_transaction::InstallIdentity::workspace(&workspace).unwrap();
    let sync = identity.acquire_sync().unwrap();
    assert!(
        request.prepare_instructions().is_err(),
        "launch must acquire sync ownership"
    );
    drop(sync);
    let install = crate::skills::install_transaction::LockedInstall::acquire(&identity).unwrap();
    assert!(
        request.prepare_instructions().is_err(),
        "launch must acquire install ownership"
    );
    drop(install);
    request.prepare_instructions().unwrap();
}

#[test]
fn all_frontends_use_one_prepared_generation_across_concurrent_publication() {
    use crate::agent::{
        AgentFrontend, AgentKind, ClaudeFrontend, CodexFrontend, OpenCodeFrontend, PiFrontend,
    };
    for kind in AgentKind::ALL {
        let home = tempfile::tempdir().unwrap();
        let workspace = workspace(home.path());
        extensions(&workspace, "OLD");
        crate::skills::install::sync_workspace(
            &workspace,
            &crate::skills::real_sources(&workspace),
        )
        .unwrap();
        let prepared = request(&workspace).prepare_instructions().unwrap();
        let ready = Arc::new(std::sync::Barrier::new(2));
        let publisher = {
            let ready = Arc::clone(&ready);
            let workspace = Arc::clone(&workspace);
            std::thread::spawn(move || {
                ready.wait();
                extensions(&workspace, "NEW");
                crate::skills::install::sync_workspace(
                    &workspace,
                    &crate::skills::real_sources(&workspace),
                )
                .unwrap();
            })
        };
        ready.wait();
        publisher.join().unwrap();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let frontend: Box<dyn AgentFrontend> = match kind {
            AgentKind::Claude => Box::new(ClaudeFrontend::new(
                fixture.join("claude/fake_claude.sh").display().to_string(),
                home.path().into(),
                home.path().join("projects"),
            )),
            AgentKind::Codex => Box::new(CodexFrontend::new(
                fixture.join("codex/fake_codex.sh").display().to_string(),
            )),
            AgentKind::OpenCode => Box::new(OpenCodeFrontend::for_workspace(
                fixture
                    .join("opencode/capture_opencode.sh")
                    .display()
                    .to_string(),
                workspace.root(),
            )),
            AgentKind::Pi => Box::new(PiFrontend::for_workspace(
                "printf '%s\\0'",
                workspace.root(),
            )),
        };
        let spec = frontend.launch_spec(&prepared).unwrap();
        let payload = format!("{} {:?}", spec.command, spec.environment);
        assert!(payload.contains("OLD workspace"), "{kind:?}");
        assert!(!payload.contains("NEW workspace"), "{kind:?}");
        let rendered = prepared
            .render_capabilities(prepared.access_policy().capability_plan().unwrap())
            .unwrap()
            .rendered_dir;
        for (path, marker) in [
            ("agenda/SKILL.md", "anchor"),
            ("agenda/references/legacy-task-context.md", "task context"),
            ("custom/SKILL.md", "custom"),
        ] {
            let body = fs::read_to_string(rendered.join(path)).unwrap();
            assert!(
                body.contains(&format!("OLD {marker}")),
                "{kind:?}: {path} mixed generations"
            );
            assert!(!body.contains("NEW"), "{kind:?}: {path} mixed generations");
        }
        assert_generation(&request(&workspace), "NEW", "OLD");
    }
}

#[path = "generation_tests/catalog.rs"]
mod catalog;

#[test]
fn launch_refuses_durable_receiving_state() {
    let home = tempfile::tempdir().unwrap();
    let workspace = workspace(home.path());
    crate::skills::install::sync_workspace(&workspace, &crate::skills::real_sources(&workspace))
        .unwrap();
    let journal = crate::sync::journal::Journal::open(&workspace.paths().sync_journal()).unwrap();
    journal
        .begin_receive(&workspace.id().to_string(), "fixture")
        .unwrap();
    assert!(request(&workspace).prepare_instructions().is_err());
}
