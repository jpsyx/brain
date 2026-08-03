use brain::personalization::model::Personalization;
use brain::state::PanelSide;
use brain::workspace::{RegistryStore, WorkspaceName};

use crate::support::{
    Fixture, snapshot_tree, sync_run, write_csv_baseline, write_session_response,
};

#[test]
fn uuid_scoped_state_locks_responses_and_sync_artifacts_do_not_collide() {
    let fixture = Fixture::new();
    let personal = &fixture.personal.workspace;
    let family = &fixture.family.workspace;

    std::fs::create_dir_all(family.root().join(".config")).expect("family config dir");
    std::fs::write(
        family.root().join(".config/config.json"),
        b"{\"sentinel\":\"family-config\"}\n",
    )
    .expect("family config");
    std::fs::write(
        family.root().join(".config/personalization.json"),
        b"{\"name\":\"Family\"}\n",
    )
    .expect("family personalization");

    let family_db = brain::state::Db::open(family).expect("family state");
    family_db
        .set_panel_side(PanelSide::Left)
        .expect("family layout");
    drop(family_db);
    write_session_response(family, "family-session", b"family response\n");
    let family_journal = brain::sync::journal::Journal::open(&family.paths().sync_journal())
        .expect("family journal");
    family_journal
        .record(&sync_run("family"))
        .expect("family journal row");
    drop(family_journal);
    write_csv_baseline(family, "family-task");
    let family_reporter = brain::sync::current::Reporter::begin(
        family.paths(),
        "both",
        "2026-08-02T00:00:00Z",
        std::process::id(),
    );
    family_reporter.line("family progress");
    let family_sync_lock =
        brain::sync::lock::try_acquire(&family.paths().sync_lock()).expect("family sync lock");
    let family_tui_lock = brain::tui::singleton::Guard::acquire(family).expect("family TUI lock");
    assert!(
        brain::tui::singleton::Guard::acquire(family).is_err(),
        "the same UUID cannot own two TUI locks"
    );

    let family_record_before = serde_json::to_vec(
        &RegistryStore::load_from(fixture.store.path())
            .expect("registry")
            .workspaces[&WorkspaceName::parse("family").expect("family")],
    )
    .expect("family record bytes");
    let family_portable_before = snapshot_tree(family.root());
    let family_runtime_before = snapshot_tree(family.paths().cache_dir());

    let personal_db = brain::state::Db::open(personal).expect("personal state");
    personal_db
        .set_panel_side(PanelSide::Right)
        .expect("personal layout");
    drop(personal_db);
    write_session_response(personal, "personal-session-1", b"personal response one\n");
    let personal_journal = brain::sync::journal::Journal::open(&personal.paths().sync_journal())
        .expect("personal journal");
    personal_journal
        .record(&sync_run("personal-before-default-change"))
        .expect("personal journal row");
    drop(personal_journal);
    write_csv_baseline(personal, "personal-task-1");
    let personal_reporter = brain::sync::current::Reporter::begin(
        personal.paths(),
        "pull",
        "2026-08-02T01:00:00Z",
        std::process::id(),
    );
    personal_reporter.line("personal progress one");
    let personal_sync_lock =
        brain::sync::lock::try_acquire(&personal.paths().sync_lock()).expect("personal sync lock");
    let personal_tui_lock =
        brain::tui::singleton::Guard::acquire(personal).expect("personal TUI lock");
    assert_eq!(snapshot_tree(family.root()), family_portable_before);
    assert_eq!(
        snapshot_tree(family.paths().cache_dir()),
        family_runtime_before
    );
    drop(personal_tui_lock);
    drop(personal_sync_lock);
    drop(personal_reporter);

    let mut registry = RegistryStore::load_from(fixture.store.path()).expect("registry");
    registry.set_default("family").expect("change default");
    fixture.store.replace(&registry).expect("persist default");

    brain::env::set(&fixture.personal, "claude_cmd", "personal-after-default")
        .expect("post-default env write");
    brain::settings::set(&fixture.personal.workspace, "day_rollover_hour", "6")
        .expect("post-default config write");
    brain::personalization::store::save(
        &fixture.personal.workspace,
        &Personalization {
            name: "Personal after default".to_owned(),
            ..Personalization::default()
        },
    )
    .expect("post-default personalization write");
    let personal_db = brain::state::Db::open(personal).expect("post-default personal state");
    personal_db
        .set_panel_side(PanelSide::Left)
        .expect("post-default layout");
    drop(personal_db);
    write_session_response(personal, "personal-session-2", b"personal response two\n");
    let personal_journal = brain::sync::journal::Journal::open(&personal.paths().sync_journal())
        .expect("post-default personal journal");
    personal_journal
        .record(&sync_run("personal-after-default-change"))
        .expect("post-default journal row");
    drop(personal_journal);
    write_csv_baseline(personal, "personal-task-2");
    let personal_reporter = brain::sync::current::Reporter::begin(
        personal.paths(),
        "push",
        "2026-08-02T02:00:00Z",
        std::process::id(),
    );
    personal_reporter.line("personal progress two");
    let personal_sync_lock = brain::sync::lock::try_acquire(&personal.paths().sync_lock())
        .expect("post-default personal sync lock");
    let personal_tui_lock =
        brain::tui::singleton::Guard::acquire(personal).expect("post-default personal TUI lock");

    let family_record_after = serde_json::to_vec(
        &RegistryStore::load_from(fixture.store.path())
            .expect("registry after selected writes")
            .workspaces[&WorkspaceName::parse("family").expect("family")],
    )
    .expect("family record bytes after selected writes");
    assert_eq!(family_record_after, family_record_before);
    assert_eq!(snapshot_tree(family.root()), family_portable_before);
    assert_eq!(
        snapshot_tree(family.paths().cache_dir()),
        family_runtime_before
    );
    assert_eq!(
        brain::state::Db::open(family)
            .expect("family state after selected writes")
            .get_panel_side(),
        PanelSide::Left
    );

    drop(personal_tui_lock);
    drop(personal_sync_lock);
    drop(personal_reporter);
    drop(family_tui_lock);
    drop(family_sync_lock);
    drop(family_reporter);
}
