//! Sync command handler.

use anyhow::Result;

pub fn run(args: &crate::cli::SyncArgs, root: &std::path::Path) -> Result<()> {
    use crate::cli::SyncAction;
    use crate::sync::args::Direction;
    let cfg = crate::sync::config::SyncConfig::load();
    match &args.action {
        Some(SyncAction::Setup) => {
            crate::logging::log("sync setup");
            crate::sync::setup::run()
        }
        Some(SyncAction::Repair) => {
            crate::logging::log("sync repair");
            run_once(&cfg, root, Direction::Resync, args.if_idle)
        }
        Some(SyncAction::Init) => {
            let theme = crate::theme::Theme::active();
            eprintln!(
                "{}",
                theme.warning(
                    "`brain sync init` was renamed to `brain sync repair`; running repair now."
                )
            );
            crate::logging::log("sync init alias -> repair");
            run_once(&cfg, root, Direction::Resync, args.if_idle)
        }
        Some(SyncAction::Status) => {
            crate::logging::log("sync status");
            crate::sync::command::print_status(&cfg, root)
        }
        Some(SyncAction::Conflicts { json }) => {
            crate::logging::log(format!("sync conflicts json={json}"));
            crate::sync::command::print_conflicts(root, *json)
        }
        Some(SyncAction::Resolve { originals }) => {
            crate::logging::log(format!("sync resolve originals={originals:?}"));
            crate::sync::command::resolve(root, originals)
        }
        None => {
            let direction = crate::sync::command::direction_from_flags(args.push, args.pull)?;
            crate::logging::log(format!(
                "sync run direction={} if_idle={}",
                crate::sync::command::direction_label(direction),
                args.if_idle
            ));
            run_once(&cfg, root, direction, args.if_idle)
        }
    }
}

fn run_once(
    config: &crate::sync::config::SyncConfig,
    root: &std::path::Path,
    direction: crate::sync::args::Direction,
    if_idle: bool,
) -> Result<()> {
    if !config.is_configured() {
        crate::logging::log("sync not configured");
        println!(
            "{}",
            crate::sync::command::format_unconfigured_sync_guidance(
                direction,
                crate::theme::Theme::active(),
            )
        );
        return Ok(());
    }
    crate::logging::log(format!(
        "sync acquire lock {}",
        crate::sync::lock::default_path().display()
    ));
    let Some(_guard) = crate::sync::lock::try_acquire(&crate::sync::lock::default_path()) else {
        if if_idle {
            crate::logging::log("sync lock busy; if-idle coalesce");
            return Ok(());
        }
        crate::logging::log("sync lock busy; following in-flight sync");
        crate::sync::follow::follow_until_done();
        return Ok(());
    };
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    crate::logging::log(format!("sync start ts={timestamp}"));
    let outcome =
        crate::sync::command::sync_once(config, root, direction, (&timestamp, &timestamp, &date))?;
    crate::logging::log(format!("sync outcome={}", outcome.label()));
    match outcome {
        crate::sync::verify::Outcome::Clean => println!("sync complete."),
        crate::sync::verify::Outcome::NeedsAttention(message)
        | crate::sync::verify::Outcome::Aborted(message) => eprintln!("{message}"),
    }
    Ok(())
}
