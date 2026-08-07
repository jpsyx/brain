use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::{Duration, Instant};

use brain::users::{PhoneIdentity, User, UserId, Users, UsersStore};
use brain::workspace::{CommandContext, RegistryStore, WorkspaceContext, WorkspaceName};

use crate::receiver_workspace_support::{DualWorkspaceReceiverFixture, poll_until};

pub(crate) struct Scenario {
    pub(crate) home: PathBuf,
    pub(crate) personal: Arc<WorkspaceContext>,
    pub(crate) family: Arc<WorkspaceContext>,
    pub(crate) store: RegistryStore,
}

pub(crate) fn prepare(fixture: &DualWorkspaceReceiverFixture) -> Scenario {
    let home = fixture.home().to_path_buf();
    let store = RegistryStore::from_path(home.join(".config/brain/env.json"));
    let mut registry = RegistryStore::load_from(store.path()).expect("receiver registry");
    let personal_name = WorkspaceName::parse("personal").expect("personal name");
    let family_name = WorkspaceName::parse("family").expect("family name");

    let personal_record = registry
        .workspaces
        .get_mut(&personal_name)
        .expect("personal record");
    "pablo".clone_into(&mut personal_record.local_user_id);
    personal_record.env.insert(
        "agent_capabilities".to_owned(),
        serde_json::json!({
            "mcps": [{
                "name": "personal-notes",
                "url": "https://personal.example.test/mcp",
                "credentials": {"bearer_token": "personal-capability-secret"}
            }]
        }),
    );

    let family_record = registry
        .workspaces
        .get_mut(&family_name)
        .expect("family record");
    family_record.aliases = BTreeSet::from([WorkspaceName::parse("fam").expect("family alias")]);
    "pablo".clone_into(&mut family_record.local_user_id);
    family_record.env.insert(
        "agent_capabilities".to_owned(),
        serde_json::json!({
            "mcps": [{
                "name": "family-notes",
                "url": "https://family.example.test/mcp",
                "credentials": {"bearer_token": "family-capability-secret"}
            }]
        }),
    );
    family_record.env.insert(
        "opencode_cmd".to_owned(),
        serde_json::Value::String(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/opencode/fake_opencode.sh")
                .display()
                .to_string(),
        ),
    );
    store.replace(&registry).expect("acceptance registry");

    let personal = Arc::new(context(&home, &fixture.personal, "pablo"));
    let family = Arc::new(context(&home, &fixture.family, "pablo"));
    std::fs::create_dir_all(personal.root().join("tasks")).expect("personal tasks directory");
    std::fs::create_dir_all(family.root().join("tasks")).expect("family tasks directory");
    UsersStore::save(&personal, &users(&[("pablo", "Pablo", true)])).expect("personal users");
    UsersStore::save(
        &family,
        &users(&[("pablo", "Pablo", false), ("wife", "Wife", true)]),
    )
    .expect("family users");
    write_config(
        &personal,
        r#"{"access_mode":"unrestricted","enable_triage_habits":true}"#,
    );
    write_config(
        &family,
        r#"{"access_mode":"workspace_only","allowed_mcps":["family-notes"],"allowed_skills":[],"enable_triage_habits":false}"#,
    );

    Scenario {
        home,
        personal,
        family,
        store,
    }
}

pub(crate) fn assert_selector_cli(scenario: &Scenario) {
    let default = run(scenario, &["config", "get", "access-mode"]);
    assert_success(&default);
    assert_eq!(
        String::from_utf8_lossy(&default.stdout).trim(),
        "unrestricted"
    );

    let family = run(scenario, &["config", "get", "access-mode", "-b", "fam"]);
    assert_success(&family);
    assert_eq!(
        String::from_utf8_lossy(&family.stdout).trim(),
        "workspace_only"
    );
}

pub(crate) fn poll_family_jobs(
    fixture: &mut DualWorkspaceReceiverFixture,
    expected: usize,
) -> Vec<brain::server::receiver::InboundJob> {
    poll_until(Instant::now() + Duration::from_secs(3), || {
        fixture.family_jobs().len() >= expected
    });
    fixture.family_jobs()
}

fn context(home: &std::path::Path, source: &WorkspaceContext, local: &str) -> WorkspaceContext {
    WorkspaceContext::new(
        home,
        source.id(),
        source.name().clone(),
        source.root(),
        local,
        home,
    )
    .expect("selected acceptance context")
}

fn users(entries: &[(&str, &str, bool)]) -> Users {
    Users {
        schema_version: brain::users::USERS_SCHEMA_VERSION,
        users: entries
            .iter()
            .map(|(id, name, owns_phone)| User {
                id: UserId::parse(id).expect("user id"),
                name: (*name).to_owned(),
                phones: owns_phone
                    .then(|| PhoneIdentity {
                        value: "+12125550100".to_owned(),
                        inbound_allowed: true,
                    })
                    .into_iter()
                    .collect(),
                emails: Vec::new(),
                response_email: None,
            })
            .collect(),
    }
}

fn write_config(workspace: &WorkspaceContext, body: &str) {
    let path = workspace.root().join(".config/config.json");
    std::fs::create_dir_all(path.parent().expect("config parent")).expect("config directory");
    std::fs::write(path, format!("{body}\n")).expect("portable config");
}

fn run(scenario: &Scenario, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(args)
        .env("HOME", &scenario.home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env("NO_COLOR", "1")
        .output()
        .expect("run acceptance CLI")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn command_context(scenario: &Scenario) -> CommandContext {
    CommandContext::new(Arc::clone(&scenario.family), scenario.store.clone())
        .expect("family command context")
}
