//! The contacts book: id assignment, refusal to guess, and search.

use chrono::NaiveDate;

use super::{Fields, add, delete, edit, find, get, list, model, render, search};

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date")
}

fn named(name: &str) -> Fields {
    Fields {
        name: Some(name.to_owned()),
        ..Fields::default()
    }
}

fn book() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[test]
fn ids_are_assigned_in_sequence_and_zero_padded() {
    let dir = book();
    let root = dir.path();

    assert_eq!(
        add(root, &named("Ada"), today())
            .expect("add")
            .contact
            .get("id"),
        "C001"
    );
    assert_eq!(
        add(root, &named("Grace"), today())
            .expect("add")
            .contact
            .get("id"),
        "C002"
    );
}

#[test]
fn a_gap_does_not_reuse_an_id() {
    let dir = book();
    let root = dir.path();
    add(root, &named("Ada"), today()).expect("add");
    add(root, &named("Grace"), today()).expect("add");
    delete(root, "C001").expect("delete");

    // Reusing C001 would silently re-point anything that referenced the old
    // contact at a different person.
    assert_eq!(
        add(root, &named("Alan"), today())
            .expect("add")
            .contact
            .get("id"),
        "C003"
    );
}

#[test]
fn adding_requires_a_name() {
    let dir = book();
    let error = add(dir.path(), &Fields::default(), today()).expect_err("no name");
    assert!(error.to_string().contains("--name is required"), "{error}");
}

#[test]
fn a_new_contact_is_stamped_on_both_dates() {
    let dir = book();
    let mutation = add(dir.path(), &named("Ada"), today()).expect("add");
    assert_eq!(mutation.contact.get("created_date"), "2026-08-24");
    assert_eq!(mutation.contact.get("last_updated"), "2026-08-24");
}

#[test]
fn an_unknown_communication_preference_is_refused() {
    let dir = book();
    let fields = Fields {
        preferred_comms: Some("carrier pigeon".to_owned()),
        ..named("Ada")
    };

    let error = add(dir.path(), &fields, today()).expect_err("bad preference");

    assert!(error.to_string().contains("--preferred-comms"), "{error}");
    // Nothing was written.
    assert!(list(dir.path(), None, None).expect("list").is_empty());
}

#[test]
fn editing_touches_only_last_updated_and_the_named_fields() {
    let dir = book();
    let root = dir.path();
    add(root, &named("Ada Lovelace"), today()).expect("add");

    let later = NaiveDate::from_ymd_opt(2026, 9, 1).expect("date");
    let mutation = edit(
        root,
        "Ada",
        &Fields {
            phone: Some("+1 555 0100".to_owned()),
            ..Fields::default()
        },
        later,
    )
    .expect("edit");

    assert_eq!(mutation.contact.get("phone"), "+1 555 0100");
    assert_eq!(mutation.contact.get("name"), "Ada Lovelace");
    assert_eq!(mutation.contact.get("created_date"), "2026-08-24");
    assert_eq!(mutation.contact.get("last_updated"), "2026-09-01");
}

#[test]
fn editing_nothing_is_refused_rather_than_stamped() {
    let dir = book();
    add(dir.path(), &named("Ada"), today()).expect("add");

    let error = edit(dir.path(), "Ada", &Fields::default(), today()).expect_err("no fields");

    assert!(error.to_string().contains("no fields given"), "{error}");
}

#[test]
fn an_ambiguous_name_is_refused_rather_than_guessed() {
    let dir = book();
    let root = dir.path();
    add(root, &named("Ada Lovelace"), today()).expect("add");
    add(root, &named("Ada Byron"), today()).expect("add");

    let error = get(root, "Ada").expect_err("ambiguous");

    // Editing the wrong person's details is not a recoverable mistake.
    assert!(
        error.to_string().contains("matches multiple contacts"),
        "{error}"
    );
    assert!(error.to_string().contains("C001"), "{error}");
    assert!(error.to_string().contains("C002"), "{error}");
}

#[test]
fn an_exact_name_wins_over_a_fragment() {
    let dir = book();
    let root = dir.path();
    add(root, &named("Ada"), today()).expect("add");
    add(root, &named("Adaline"), today()).expect("add");

    assert_eq!(get(root, "Ada").expect("exact").get("id"), "C001");
}

#[test]
fn resolution_prefers_the_id() {
    let dir = book();
    let root = dir.path();
    add(root, &named("C002"), today()).expect("add");
    add(root, &named("Grace"), today()).expect("add");

    // The row *named* "C002" is C001; asking for C002 means the id.
    assert_eq!(get(root, "C002").expect("by id").get("name"), "Grace");
}

#[test]
fn an_unknown_needle_says_so() {
    let dir = book();
    let error = get(dir.path(), "nobody").expect_err("unknown");
    assert!(error.to_string().contains("no contact matches"), "{error}");
}

#[test]
fn search_covers_every_field_by_default_and_one_when_named() {
    let dir = book();
    let root = dir.path();
    add(
        root,
        &Fields {
            job: Some("Accountant".to_owned()),
            ..named("Ada")
        },
        today(),
    )
    .expect("add");
    add(
        root,
        &Fields {
            notes: Some("recommended by our accountant".to_owned()),
            ..named("Grace")
        },
        today(),
    )
    .expect("add");

    assert_eq!(search(root, "accountant", None).expect("search").len(), 2);
    let by_job = search(root, "accountant", Some("job")).expect("search");
    assert_eq!(by_job.len(), 1);
    assert_eq!(by_job[0].get("name"), "Ada");
}

#[test]
fn tags_match_whole_values_not_fragments() {
    let dir = book();
    let root = dir.path();
    add(
        root,
        &Fields {
            tags: Some("family;medical".to_owned()),
            ..named("Ada")
        },
        today(),
    )
    .expect("add");

    assert_eq!(list(root, Some("family"), None).expect("list").len(), 1);
    assert_eq!(list(root, Some("MEDICAL"), None).expect("list").len(), 1);
    assert!(
        list(root, Some("fam"), None).expect("list").is_empty(),
        "a tag fragment is not a tag"
    );
}

#[test]
fn the_book_is_stored_in_id_order() {
    let dir = book();
    let root = dir.path();
    for name in ["Ada", "Grace", "Alan"] {
        add(root, &named(name), today()).expect("add");
    }
    delete(root, "C002").expect("delete");
    add(root, &named("Katherine"), today()).expect("add");

    let text = std::fs::read_to_string(model::csv_path(root)).expect("read book");
    let ids: Vec<&str> = text
        .lines()
        .skip(1)
        .filter_map(|line| line.split(',').next())
        .collect();
    assert_eq!(ids, ["C001", "C003", "C004"]);
}

#[test]
fn the_book_is_scoped_to_the_workspace_it_is_given() {
    let one = book();
    let two = book();
    add(one.path(), &named("Ada"), today()).expect("add");

    // The script this replaced resolved `~/brain` directly, so a second
    // workspace read the first one's book.
    assert!(list(two.path(), None, None).expect("list").is_empty());
}

#[test]
fn an_empty_table_says_so() {
    assert_eq!(render::table(&[]), "(no matching contacts)\n");
}

#[test]
fn the_table_aligns_every_column() {
    let dir = book();
    let root = dir.path();
    add(
        root,
        &Fields {
            job: Some("Accountant".to_owned()),
            ..named("Ada")
        },
        today(),
    )
    .expect("add");

    let out = render::table(&list(root, None, None).expect("list"));
    let lines: Vec<&str> = out.lines().collect();

    assert!(lines[0].starts_with("id    name"), "{out}");
    assert!(lines[1].starts_with("----"), "{out}");
    assert!(lines[2].starts_with("C001  Ada   Accountant"), "{out}");
}

#[test]
fn a_missing_fallback_configuration_is_an_error_not_an_empty_answer() {
    let dir = book();
    let error = super::fallback(dir.path()).expect_err("no fallback");
    assert!(
        error.to_string().contains("no fallback directory"),
        "{error}"
    );
}

#[test]
fn a_configured_fallback_is_returned_verbatim() {
    let dir = book();
    let root = dir.path();
    std::fs::create_dir_all(root.join("resources/contacts")).expect("dir");
    std::fs::write(
        model::config_path(root),
        r#"{"notion_fallback":{"database_id":"abc","name":"People"}}"#,
    )
    .expect("config");

    let fallback = super::fallback(root).expect("fallback");

    // Core carries the block opaquely; what the service is, is the caller's.
    assert_eq!(fallback["database_id"], "abc");
    assert_eq!(fallback["name"], "People");
}

#[test]
fn resolution_reports_how_it_matched() {
    let dir = book();
    let root = dir.path();
    add(root, &named("Ada Lovelace"), today()).expect("add");
    let rows = model::load(root).expect("load");

    assert_eq!(
        find::resolve(&rows, "C001").expect("id").1,
        find::Matched::Id
    );
    assert_eq!(
        find::resolve(&rows, "ada lovelace").expect("name").1,
        find::Matched::ExactName
    );
    assert_eq!(
        find::resolve(&rows, "lovelace").expect("fragment").1,
        find::Matched::NameFragment
    );
}
