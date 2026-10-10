use crate::cli::ShareArgs;
use crate::theme::Theme;
use crate::workspace::CommandContext;
use anyhow::{Result, bail};
use std::path::PathBuf;

pub fn run(args: &ShareArgs, command: &CommandContext) -> Result<()> {
    let theme = Theme::active();
    let path = if let Some(path) = &args.path {
        path.clone()
    } else {
        let Some(input) = super::prompt_tty_line(&theme.prompt("File or directory to share: "))?
        else {
            bail!("Provide a file or directory: brain share <path> [--expires 7d]");
        };
        PathBuf::from(input.trim())
    };
    let result = crate::share::run(
        command,
        &path,
        args.expires.as_deref().unwrap_or(""),
        &mut |phase| eprintln!("{} {}", theme.accent("Sharing"), theme.muted(phase)),
    )?;
    eprintln!(
        "{} {}",
        theme.success("Download link ready"),
        theme.value(&result.path.display().to_string())
    );
    eprintln!(
        "{} {}",
        theme.muted("Expires"),
        theme.value(&result.expires_at)
    );
    if args.json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("{}", result.url);
    }
    Ok(())
}
