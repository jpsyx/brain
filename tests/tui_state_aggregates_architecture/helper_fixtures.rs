use super::support::{
    directly_accesses_field, expected_links_plan_shape, extract_struct_body,
    field_declaration_count, field_is_private, field_type, has_aliased_field_access,
    has_exact_named_shape, has_pure_direct_aggregate_forwarder, has_raw_aggregate_forwarder,
};

#[test]
fn guard_helpers_cover_visibility_duplicates_aliases_and_forwarders() {
    let alternate_visibility = r"
        struct App {
            pub(super) query: String,
        }
    ";
    let body = extract_struct_body(alternate_visibility, "App").expect("App body");
    assert_eq!(field_declaration_count(body, "query"), 1);
    assert!(!field_is_private(body, "query"));
    assert_eq!(field_type(body, "query"), Some("String".to_owned()));

    let duplicate_visibility = r"
        struct App {
            pub(super) query: String,
            pub(in crate::tui) query: String,
        }
    ";
    let body = extract_struct_body(duplicate_visibility, "App").expect("App body");
    assert_eq!(field_declaration_count(body, "query"), 2);

    let exact_panel = r"
        pub(crate) struct TasksPanelModel<'a> {
            state: &'a TasksState,
        }
    ";
    let raw_assignment_field = r"
        pub(crate) struct TaskAssignmentSnapshot<'a> {
            pub(crate) mode: AssignmentUiMode,
            pub(crate) actor_id: &'a UserId,
            pub(crate) users: &'a [AssignmentUser],
            pub(crate) filter: Option<&'a UserId>,
            pub(crate) task: &'a Task,
        }
    ";
    let extra_panel_field = r"
        pub(crate) struct TasksPanelModel<'a> {
            state: &'a TasksState,
            count: usize,
        }
    ";
    let raw_nudge_field = r"
        pub(crate) struct DailyTriageNudge {
            pub(crate) task_id: String,
            pub(crate) task_label: String,
            pub(crate) habit: Task,
        }
    ";
    let extra_links_variant = r"
        pub(crate) enum TaskLinksPlan {
            None,
            Open { url: String },
            Choose { task_id: String, links: Vec<Link> },
            Raw(Task),
        }
    ";
    assert!(has_exact_named_shape(
        exact_panel,
        "struct",
        "TasksPanelModel",
        "state:&'aTasksState,"
    ));
    assert!(!has_exact_named_shape(
        raw_assignment_field,
        "struct",
        "TaskAssignmentSnapshot",
        "pub(crate)mode:AssignmentUiMode,pub(crate)actor_id:&'aUserId,pub(crate)users:&'a[AssignmentUser],pub(crate)filter:Option<&'aUserId>,"
    ));
    assert!(!has_exact_named_shape(
        extra_panel_field,
        "struct",
        "TasksPanelModel",
        "state:&'aTasksState,"
    ));
    assert!(!has_exact_named_shape(
        raw_nudge_field,
        "struct",
        "DailyTriageNudge",
        "pub(crate)task_id:String,pub(crate)task_label:String,"
    ));
    assert!(!has_exact_named_shape(
        extra_links_variant,
        "enum",
        "TaskLinksPlan",
        &expected_links_plan_shape()
    ));

    assert!(directly_accesses_field(
        "fn leak(app: &App) { let _ = &app.tasks.query; }",
        "tasks",
        "query"
    ));

    let alias_leak = r"
        fn leak(app: &App) {
            let local = &app.tasks;
            let _ = &local.query;
        }
    ";
    assert!(has_aliased_field_access(alias_leak, "tasks", &["query"]));

    let multiline_typed_parenthesized_alias = r"
        fn leak(app: &App) {
            let local: &TasksState = (
                &app.tasks
            );
            let _ = &local.query;
        }
    ";
    assert!(has_aliased_field_access(
        multiline_typed_parenthesized_alias,
        "tasks",
        &["query"]
    ));

    let forwarding = r"
        impl App {
            fn query(&self) -> &str { self.tasks.query_text() }
        }
    ";
    assert!(has_raw_aggregate_forwarder(forwarding));

    let multi_statement_forwarding = r"
        impl App {
            fn query(&self) -> &str {
                let state: &TasksState = (&self.tasks);
                state.query_text()
            }
        }
    ";
    assert!(has_raw_aggregate_forwarder(multi_statement_forwarding));

    let intermediate_forwarding = r"
        impl App {
            fn query(&self) -> &str {
                let state: &TasksState = (&self.tasks);
                let value: &str = ((state.query_text()));
                ((value))
            }
        }
    ";
    assert!(has_raw_aggregate_forwarder(intermediate_forwarding));

    let aliased_raw_return = r"
        type BorrowedValue<'a> = &'a str;
        type RenamedValue<'a> = BorrowedValue<'a>;

        impl App {
            fn query(&self) -> RenamedValue<'_> {
                self.tasks.query_text()
            }
        }
    ";
    assert!(has_raw_aggregate_forwarder(aliased_raw_return));

    let renamed_context_forwarder = r"
        impl App {
            fn endpoint(&self, port: u16) -> String {
                self.context.session_done_url(port)
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        renamed_context_forwarder
    ));

    let chained_brain_forwarder = r"
        impl App {
            fn panel_open(&self) -> bool {
                self.brain.main_controller().is_some()
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(chained_brain_forwarder));

    let aliased_context_forwarder = r"
        impl App {
            fn endpoint(&self, port: u16) -> String {
                let context = &self.context;
                context.session_done_url(port)
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        aliased_context_forwarder
    ));

    let propagated_typed_parenthesized_brain_forwarder = r"
        impl App {
            fn panel_open(&self) -> bool {
                let state: &BrainPanelState = ((&self.brain));
                let controller = (state.main_controller());
                ((controller.is_some()))
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        propagated_typed_parenthesized_brain_forwarder
    ));

    let dead_aliases_before_context_forwarder = r"
        impl App {
            fn endpoint(&self, port: u16) -> String {
                let unused_tasks = &self.tasks;
                let unused_shell = &self.shell;
                let context = &self.context;
                context.session_done_url(port)
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        dead_aliases_before_context_forwarder
    ));

    let shadowed_alias_uses_the_last_binding = r"
        impl App {
            fn endpoint(&self, port: u16) -> String {
                let owner = &self.brain;
                let owner: &AppContext = ((&self.context));
                owner.session_done_url(port)
            }
        }
    ";
    assert!(has_pure_direct_aggregate_forwarder(
        shadowed_alias_uses_the_last_binding
    ));

    let cross_aggregate_mediator = r"
        impl App {
            fn active_tab(&self) -> BrainTab {
                self.shell.active_brain_tab(&self.brain.skill_session_tab_ids())
            }
        }
    ";
    assert!(!has_pure_direct_aggregate_forwarder(
        cross_aggregate_mediator
    ));

    let aliased_cross_aggregate_mediator = r"
        impl App {
            fn active_tab(&self) -> BrainTab {
                let shell = &self.shell;
                let brain: &BrainPanelState = ((&self.brain));
                shell.active_brain_tab(&brain.skill_session_tab_ids())
            }
        }
    ";
    assert!(!has_pure_direct_aggregate_forwarder(
        aliased_cross_aggregate_mediator
    ));
}
