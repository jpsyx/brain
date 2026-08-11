include!("tests_support.rs");
include!("tests_parts/part_01.rs");
include!("tests_parts/part_02.rs");
include!("tests_parts/part_03.rs");
include!("tests_parts/part_04.rs");
include!("tests_parts/part_05.rs");
include!("tests_parts/part_06.rs");

#[test]
fn the_task_state_probe_lists_only_the_tasks_directory() {
    // It needs three known paths. Listing the whole bucket recursively to find
    // them cost ~2s on a 6.7k-object remote, once per sync.
    let temporary = tempfile::tempdir().expect("staging");
    let mut commands: Vec<Vec<String>> = Vec::new();
    let state = super::fetch_remote_task_schema_with("BRAIN:bucket", temporary.path(), |args| {
        commands.push(args.to_vec());
        (true, "habits.csv\ntasks.csv\n.tasks_next_id\n".to_owned())
    })
    .expect("probe the remote task state");

    assert!(state.has_csvs);
    assert_eq!(state.schema, None, "no SCHEMA.json was listed");
    let listing = commands.first().expect("a listing command ran");
    assert_eq!(listing.first().map(String::as_str), Some("lsf"));
    assert_eq!(
        listing.get(1).map(String::as_str),
        Some("BRAIN:bucket/tasks")
    );
    assert!(
        !listing.iter().any(|argument| argument == "--recursive"),
        "the probe must not walk the whole remote: {listing:?}"
    );
}

#[test]
fn a_tasks_directory_without_csvs_reports_none_present() {
    let temporary = tempfile::tempdir().expect("staging");
    let state = super::fetch_remote_task_schema_with("BRAIN:bucket", temporary.path(), |_| {
        (true, "\n".to_owned())
    })
    .expect("probe an empty tasks directory");

    assert!(!state.has_csvs);
    assert_eq!(state.schema, None);
}
