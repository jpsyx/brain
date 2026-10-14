//! `brain` binary entry point.

use anyhow::Result;

fn main() -> Result<()> {
    let mut cli = brain::cli::parse();
    let selected_agent = match cli.selected_agent() {
        Ok(selected_agent) => selected_agent,
        Err(error) => {
            eprintln!(
                "{}",
                brain::theme::Theme::active().error_line("🔴", &error.to_string())
            );
            std::process::exit(2);
        }
    };

    if cli.print_version || matches!(&cli.command, Some(brain::cli::Cmd::Version)) {
        print!("{}", brain::cli::version_line());
        return Ok(());
    }

    if let Some(brain::cli::Cmd::InternalMigration(args)) = &cli.command {
        brain::startup_migration::run_explicit(&args.from_version, &args.to_version)?;
        return Ok(());
    }

    if brain::skills::repair::is_requested(&cli) {
        let code = brain::skills::repair::run(&cli)?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }

    let removal_json = brain::command::tasks::removal_mode(&cli).unwrap_or(false);

    if let Err(error) = brain::startup_migration::run_current() {
        exit_with_error(&error, None, removal_json);
    }

    // An explicitly selected frontend is validated before any workspace, TUI,
    // hook, server, or PTY setup. A frontend that comes from workspace env can
    // only be known after bootstrap, so it is validated inside dispatch.
    if let Some(agent_kind) = selected_agent
        && let Err(error) = brain::command::dispatch::validate_agent_kind(agent_kind)
    {
        eprintln!(
            "{}",
            brain::theme::Theme::active().error_line("🔴", &error.to_string())
        );
        std::process::exit(2);
    }

    let read_only_status = brain::workspace::is_read_only_status(&cli);
    let log_guard = match (!read_only_status)
        .then(|| brain::logging::init(cli.verbose, !removal_json))
        .transpose()
    {
        Ok(guard) => guard,
        Err(error) => exit_with_error(&error.into(), None, removal_json),
    };
    if !read_only_status {
        let argv = std::env::args().collect::<Vec<String>>();
        brain::logging::log(format!("argv {:?}", brain::logging::redact_argv(&argv)));
    }

    let bootstrap = match brain::workspace::bootstrap(&mut cli) {
        Ok(context) => context,
        Err(error) => exit_with_error(
            &brain::workspace::command::render_error(error),
            log_guard,
            removal_json,
        ),
    };

    let agent_kind = brain::agent::resolved_frontend(selected_agent, &bootstrap);
    if let Err(error) = brain::command::dispatch::run(cli, agent_kind, &bootstrap) {
        exit_with_error(&error, log_guard, removal_json);
    }
    Ok(())
}

fn exit_with_error(
    error: &anyhow::Error,
    log_guard: Option<brain::logging::Guard>,
    removal_json: bool,
) -> ! {
    let code = error.downcast_ref::<brain::command::tasks::RenderedExit>().map_or_else(|| {
        if removal_json {
            println!("{}", serde_json::json!({
                "schema_version": 1, "kind": "error", "code": "command_failed",
                "message": "task removal could not complete command setup or dispatch", "details": {}
            }));
        }
        eprintln!("{error}");
        1
    }, |rendered| rendered.0);
    drop(log_guard);
    std::process::exit(code);
}
