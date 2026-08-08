include!("support/workspace_docs_support.rs");

#[test]
fn documented_workspace_commands_and_selectors_exist_in_clap() {
    let root_help = brain_help(&["--help"]);
    assert!(root_help.contains("-w, --workspace <WORKSPACE>"));
    assert!(root_help.contains("workspace"));

    let workspace_help = brain_help(&["workspace", "--help"]);
    let docs = current_docs();
    for command in [
        "list", "create", "attach", "rename", "alias", "default", "remove", "repair",
    ] {
        assert!(
            workspace_help
                .lines()
                .any(|line| line.trim_start().starts_with(command)),
            "workspace help is missing {command}"
        );
        assert!(
            docs.contains(&format!("workspace {command}")),
            "current docs are missing workspace {command}"
        );
    }

    let alias_help = brain_help(&["workspace", "alias", "--help"]);
    for command in ["add", "remove"] {
        assert!(
            alias_help
                .lines()
                .any(|line| line.trim_start().starts_with(command)),
            "workspace alias help is missing {command}"
        );
        assert!(
            docs.contains(&format!("workspace alias {command}")),
            "current docs are missing workspace alias {command}"
        );
    }
}

#[test]
fn current_docs_pin_workspace_storage_and_reject_obsolete_root_writes() {
    let docs = current_docs();
    for location in [
        "$XDG_CONFIG_HOME/brain/env.json",
        "<workspace-root>/.config/workspace.json",
        "~/.cache/brain/workspaces/<workspace-uuid>/",
    ] {
        assert!(
            docs.contains(location),
            "current docs are missing {location}"
        );
    }

    assert!(
        !docs.lines().any(|line| {
            let line = line.trim_start_matches([' ', '`', '$', '>']);
            line.starts_with("brain env set root=")
        }),
        "current docs must not instruct users to mutate structural roots through brain env"
    );
}

#[test]
fn current_docs_state_the_advisory_security_and_default_invariants() {
    let docs = current_docs_normalized().to_lowercase();
    for phrase in [
        "workspace_only` is advisory prompt enforcement plus best-effort capability filtering, easy to bypass, and not tenant isolation",
        "changing the default workspace never changes access mode",
    ] {
        assert!(docs.contains(phrase), "current docs are missing {phrase:?}");
    }
}

#[test]
fn docs_index_exposes_the_current_workspace_shell_contract() {
    let index = read_doc("docs/README.md");
    assert!(index.contains("`brain workspace …`"));
    assert!(index.contains("three main views"));
}

#[test]
fn current_docs_do_not_pin_selected_workspace_paths_to_brain() {
    for (path, stale_claim) in [
        ("docs/features.md", "`~/brain/tasks/{tasks,habits}.csv`"),
        ("docs/features.md", "rescope search to `~/brain/"),
        ("docs/keybindings.md", "fuzzy picker over `~/brain`"),
        ("docs/architecture.md", "`~/brain/...`\n`display`"),
    ] {
        assert!(
            !read_doc(path).contains(stale_claim),
            "{path} still contains fixed-root claim {stale_claim:?}"
        );
    }
}

#[test]
fn current_docs_scope_the_tui_singleton_by_workspace_uuid() {
    let features = read_doc("docs/features.md");
    assert!(features.contains("one live TUI per workspace UUID"));
    assert!(!features.contains("Only one interactive\n`brain` shell may run at a time"));
}

#[test]
fn docs_index_calls_paths_a_legacy_only_compatibility_module() {
    let index = read_doc("docs/README.md");
    assert!(index.contains("legacy migration-only root compatibility"));
}

#[test]
fn current_docs_distinguish_active_run_logs_from_the_reserved_workspace_path() {
    let docs = current_docs();
    assert!(docs.contains("Active run logs remain under `/tmp`"));
    assert!(docs.contains("`WorkspacePaths::logs_dir` is reserved and unused"));
}

#[test]
fn current_docs_limit_the_markdown_to_pdf_gate_to_tui_and_task_routes() {
    let features = read_doc("docs/features.md");
    assert!(features.contains("Only TUI and task routes cross this prerequisite gate"));
}

#[test]
fn data_model_lists_every_intentional_stdout_family() {
    for path in [
        "README.md",
        "docs/architecture.md",
        "docs/data-model.md",
        "docs/integrations.md",
    ] {
        let docs = read_doc_normalized(path);
        for output in [
            "`config/env/version`",
            "`workspace list`",
            "explicit plain-task output",
            "help",
            "`--verbose` mirrors logs to stdout",
            "Clap errors and diagnostics go to stderr",
            "The TUI renders to `/dev/tty`",
        ] {
            assert!(
                docs.contains(output),
                "{path} output-channel contract is missing {output:?}"
            );
        }
    }
}

#[test]
fn root_help_names_tasks_search_and_logs_as_three_main_views() {
    let help = brain_help(&["--help"]);
    assert!(help.contains("three main views"));
    for view in ["tasks", "search", "logs"] {
        assert!(help.contains(view), "root help is missing the {view} view");
    }
}

#[test]
fn root_help_names_the_current_shortcuts_binding() {
    let help = brain_help(&["--help"]);
    assert!(help.contains("Alt-S shows help"));
    assert!(!help.contains("Alt-? shows help"));
}

#[test]
fn cargo_metadata_describes_the_current_multi_workspace_agent_surface() {
    let manifest = read_doc("Cargo.toml");
    assert!(manifest.contains(
        "description = \"Multi-workspace terminal dispatch for notes, tasks, sync, and agent sessions.\""
    ));
}

#[test]
fn readme_names_tasks_search_and_logs_as_three_main_views() {
    let readme = read_doc_normalized("README.md");
    assert!(readme.contains("three main views (tasks, brain-directory search, and logs)"));
}

#[test]
fn decisions_name_tasks_search_and_logs_as_three_main_views() {
    let decisions = read_doc_normalized("docs/decisions.md");
    assert!(decisions.contains("three main views (tasks, brain-directory search, and logs)"));
}

#[test]
fn project_agent_contract_lists_current_command_and_output_families() {
    let agents = read_doc_normalized("AGENTS.md");
    for command in ["`brain workspace", "`brain env"] {
        assert!(
            agents.contains(command),
            "AGENTS.md is missing command family {command:?}"
        );
    }
    for output in [
        "`config/env/version`",
        "`workspace list`",
        "explicit plain-task output",
        "help",
        "Clap errors and diagnostics go to stderr",
        "The TUI renders to `/dev/tty`",
    ] {
        assert!(
            agents.contains(output),
            "AGENTS.md output-channel contract is missing {output:?}"
        );
    }
}

#[test]
fn invalid_commands_write_clap_errors_only_to_stderr() {
    let Output {
        status,
        stdout,
        stderr,
    } = Command::new(env!("CARGO_BIN_EXE_brain"))
        .arg("definitely-not-a-brain-command")
        .output()
        .expect("run invalid brain command");

    assert!(!status.success());
    assert!(stdout.is_empty(), "invalid command wrote to stdout");
    assert!(
        !stderr.is_empty(),
        "invalid command did not write to stderr"
    );
    assert!(String::from_utf8_lossy(&stderr).contains("error:"));
}

#[test]
fn readme_and_integrations_describe_both_execution_surfaces() {
    for path in ["README.md", "docs/integrations.md"] {
        let docs = read_doc_normalized(path);
        assert!(
            docs.contains("a persistent TUI and short-lived command families"),
            "{path} is missing the dual execution-surface contract"
        );
        assert!(
            !docs.contains("everything the user does happens inside the persistent TUI"),
            "{path} still claims every command runs in the TUI"
        );
    }
}

#[test]
fn architecture_lists_the_complete_short_lived_command_surface() {
    let architecture = read_doc_normalized("docs/architecture.md");
    assert!(architecture.contains("a persistent TUI and short-lived command families"));
    assert!(architecture.contains(
        "non-TUI task utilities, config, env, workspace, portable users, sync, personalization, skills, \
         server/receiver, habits, checks, and reindexing"
    ));
}

#[test]
fn decisions_and_docs_index_describe_both_execution_surfaces() {
    for path in ["docs/decisions.md", "docs/README.md"] {
        let docs = read_doc_normalized(path);
        assert!(
            docs.contains("a persistent TUI and short-lived command families"),
            "{path} is missing the dual execution-surface contract"
        );
        assert!(
            !docs.contains("pure TUI binary"),
            "{path} still calls Brain a pure TUI binary"
        );
    }
}

#[test]
fn architecture_distinguishes_short_lived_and_tui_task_routes() {
    let tasks_help = brain_help(&["tasks", "--help"]);
    for route in ["complete", "doctor", "search", "--no-tui"] {
        assert!(
            tasks_help.contains(route),
            "executable tasks help is missing {route}"
        );
    }

    let architecture = read_doc_normalized("docs/architecture.md");
    for contract in [
        "`brain tasks complete`, `brain tasks add`, `brain tasks set`, `brain tasks doctor`, and `brain tasks --no-tui` are short-lived",
        "`brain tasks search` opens the persistent TUI",
    ] {
        assert!(
            architecture.contains(contract),
            "architecture is missing task-routing contract {contract:?}"
        );
    }
}

#[test]
fn architecture_accounts_for_non_tui_help_and_version_exits() {
    let root_help = brain_help(&["--help"]);
    assert!(root_help.contains("-v, --version"));

    let architecture = read_doc_normalized("docs/architecture.md");
    assert!(architecture.contains("Help and version exit without opening the TUI"));
    assert!(architecture.contains(
        "After these explicit exits, bare `brain` and interactive task routes open the persistent TUI"
    ));
}

#[test]
fn readme_examples_use_only_neutral_user_identifiers() {
    let readme = read_doc("README.md");
    assert!(
        !readme.to_lowercase().contains("pablo"),
        "README contains a personal user identifier"
    );
    assert!(readme.contains("primary-user"));
}

#[test]
fn phase_two_matrix_requires_two_machine_same_person_acceptance_fixture() {
    let fixture = std::fs::read_to_string("tests/phase2_acceptance.rs").unwrap_or_default();
    let testing = std::fs::read_to_string("docs/testing.md").unwrap();
    let name = "two_machine_registries_select_the_same_portable_person";

    assert!(fixture.contains(name), "missing executable fixture {name}");
    assert!(testing.contains(name), "test matrix must cite {name}");
}

#[test]
fn phase_two_matrix_requires_inbound_actor_to_task_acceptance_fixture() {
    let fixture = std::fs::read_to_string("tests/phase2_acceptance.rs").unwrap_or_default();
    let testing = std::fs::read_to_string("docs/testing.md").unwrap();
    let name = "authenticated_inbound_actor_drives_default_task_assignment";

    assert!(fixture.contains(name), "missing executable fixture {name}");
    assert!(testing.contains(name), "test matrix must cite {name}");
}

#[test]
fn generic_agenda_after_build_hook_is_documented() {
    for path in [
        "README.md",
        "docs/architecture.md",
        "docs/features.md",
        "docs/decisions.md",
    ] {
        let doc = std::fs::read_to_string(path).unwrap();
        assert!(
            doc.contains("todo:agenda-after-build"),
            "{path} must document the generic agenda-after-build hook"
        );
    }
}

#[test]
fn readme_scopes_workspace_silos_to_persisted_artifacts() {
    let readme = read_doc_normalized("README.md");
    assert!(readme.contains("persisted state, configuration, and runtime artifacts"));
    assert!(readme.contains(
        "workspace_only` is advisory prompt enforcement plus best-effort capability filtering, easy to bypass, and not tenant isolation"
    ));
    assert!(!readme.contains("Every workspace is a silo"));
}
