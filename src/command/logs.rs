use crate::cli::LogsArgs;
use crate::theme::Theme;
use anyhow::{Context, Result, ensure};

/// Open a complete run log or copy its recent diagnostic context.
pub fn run(args: &LogsArgs) -> Result<()> {
    let theme = Theme::active();
    let path = if let Some(path) = &args.file {
        path.clone()
    } else {
        let value = super::prompt_tty_line(&theme.prompt("Brain log file path: "))?
            .context("A log file is required. Run brain logs /path/to/run.log [--copy]")?;
        ensure!(
            !value.trim().is_empty(),
            "No log file selected; nothing exported"
        );
        std::path::PathBuf::from(value.trim())
    };
    eprintln!(
        "{}",
        theme.info(if args.copy {
            "Reading recent brain logs..."
        } else {
            "Opening brain log file..."
        })
    );
    let result = if args.copy {
        crate::logging::export::recent_logs(&path)
            .and_then(|text| crate::open_target::copy_text_to_clipboard(&text))
    } else {
        crate::open_target::open_text_file(&path)
    };
    if let Err(error) = result {
        crate::logging::log(format!("log export failed: {error:#}"));
        anyhow::bail!(
            "{}\n  {}",
            theme.error("❌ Could not export the brain log."),
            theme.warning(
                "⚠️ Check the file path and text editor or clipboard availability, then retry."
            )
        );
    }
    eprintln!(
        "{}",
        theme.success(if args.copy {
            "✓ Recent brain logs copied to clipboard"
        } else {
            "✓ Opened brain log file"
        })
    );
    Ok(())
}
