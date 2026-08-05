//! `brain` binary entry point.

use anyhow::Result;

fn main() -> Result<()> {
    let mut cli = brain::cli::parse();
    let agent_kind = match cli.selected_agent() {
        Ok(agent_kind) => agent_kind,
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

    if let Err(error) = brain::command::dispatch::validate_agent_kind(agent_kind) {
        eprintln!(
            "{}",
            brain::theme::Theme::active().error_line("🔴", &error.to_string())
        );
        std::process::exit(2);
    }

    let read_only_status = brain::workspace::is_read_only_status(&cli);
    let log_guard = (!read_only_status)
        .then(|| brain::logging::init(cli.verbose, true))
        .transpose()?;
    if !read_only_status {
        let argv = std::env::args().collect::<Vec<String>>();
        brain::logging::log(format!("argv {:?}", brain::logging::redact_argv(&argv)));
    }

    let bootstrap = match brain::workspace::bootstrap(&mut cli) {
        Ok(context) => context,
        Err(error) => exit_with_error(&brain::workspace::command::render_error(error), log_guard),
    };

    if let Err(error) = brain::command::dispatch::run(cli, agent_kind, &bootstrap) {
        exit_with_error(&error, log_guard);
    }
    Ok(())
}

fn exit_with_error(error: &anyhow::Error, log_guard: Option<brain::logging::Guard>) -> ! {
    eprintln!("{error}");
    drop(log_guard);
    std::process::exit(1);
}
