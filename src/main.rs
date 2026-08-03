//! `brain` binary entry point.

use anyhow::Result;

fn main() -> Result<()> {
    let mut cli = brain::cli::parse();
    let agent_kind = cli.agent_kind();

    if cli.print_version || matches!(&cli.command, Some(brain::cli::Cmd::Version)) {
        print!("{}", brain::cli::version_line());
        return Ok(());
    }

    let log_guard = brain::logging::init(cli.verbose, true)?;
    brain::logging::log(format!(
        "argv {:?}",
        std::env::args().collect::<Vec<String>>()
    ));

    let bootstrap = match brain::workspace::bootstrap(&mut cli) {
        Ok(context) => context,
        Err(error) => exit_with_error(&brain::workspace::command::render_error(error), log_guard),
    };

    if let Err(error) = brain::command::dispatch::run(cli, agent_kind, &bootstrap) {
        exit_with_error(&error, log_guard);
    }
    Ok(())
}

fn exit_with_error(error: &anyhow::Error, log_guard: brain::logging::Guard) -> ! {
    eprintln!("{error}");
    drop(log_guard);
    std::process::exit(1);
}
