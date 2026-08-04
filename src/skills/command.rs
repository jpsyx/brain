//! `brain skills sync [--root <dir>]` — render + install the bundled skills
//! (plus the user's extensions/plugins).
//!
//! With `--root`, everything installs under a sandbox dir and reads
//! extensions/plugins from `<root>/{extensions,plugins}` (dev/tests, no touch to
//! the live registry). Without it, the real per-user layout + brain-root sources
//! are used.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::install::{self, Sources};
use super::layout::Layout;

/// Run `brain skills status` for the selected workspace.
pub fn run_status(context: &crate::workspace::CommandContext) -> Result<()> {
    let config = crate::config::Config::try_load(&context.workspace)?;
    let plan = crate::access::capability_plan_for(&config, context)?;
    println!(
        "{}",
        format_capability_status(&plan, crate::theme::Theme::active())
    );
    Ok(())
}

/// Render requested, available, and frontend enforcement without connection material.
#[must_use]
pub fn format_capability_status(
    plan: &crate::access::CapabilityPlan,
    theme: crate::theme::Theme,
) -> String {
    let mut output = theme.heading("Workspace agent capabilities");
    write!(
        output,
        "\n  {} {}",
        theme.muted("source workspace:"),
        theme.value(&plan.credentials.source_workspace().to_string())
    )
    .expect("writing to a String cannot fail");
    append_mcp_status(&mut output, plan, theme);
    append_skill_status(&mut output, plan, theme);
    output
}

fn append_mcp_status(
    output: &mut String,
    plan: &crate::access::CapabilityPlan,
    theme: crate::theme::Theme,
) {
    write!(output, "\n\n{}", theme.accent("MCP capabilities"))
        .expect("writing to a String cannot fail");
    if plan.mcps.uses_global_configuration() {
        write!(
            output,
            "\n  {}",
            theme.warning("frontend global configuration")
        )
        .expect("writing to a String cannot fail");
        return;
    }
    let claude = plan.enforcement_report(crate::access::EnforcementEvidence::strict_mcps_only());
    let codex = plan.enforcement_report(crate::access::EnforcementEvidence::advisory_only());
    for name in plan.mcps.names() {
        append_capability_row(
            output,
            name,
            plan.mcps.unavailable_reason(name),
            claude.mcps.enforcement(name),
            codex.mcps.enforcement(name),
            theme,
        );
    }
    if plan.mcps.names().is_empty() {
        write!(output, "\n  {}", theme.muted("none requested"))
            .expect("writing to a String cannot fail");
    }
}

fn append_skill_status(
    output: &mut String,
    plan: &crate::access::CapabilityPlan,
    theme: crate::theme::Theme,
) {
    write!(output, "\n\n{}", theme.accent("Skill capabilities"))
        .expect("writing to a String cannot fail");
    if plan.skills.uses_global_configuration() {
        write!(
            output,
            "\n  {}",
            theme.warning("frontend global configuration")
        )
        .expect("writing to a String cannot fail");
        return;
    }
    let report = plan.enforcement_report(crate::access::EnforcementEvidence::advisory_only());
    for name in plan.skills.names() {
        append_capability_row(
            output,
            name,
            plan.skills.unavailable_reason(name),
            report.skills.enforcement(name),
            report.skills.enforcement(name),
            theme,
        );
    }
    if plan.skills.names().is_empty() {
        write!(output, "\n  {}", theme.muted("none requested"))
            .expect("writing to a String cannot fail");
    }
}

fn append_capability_row(
    output: &mut String,
    name: &str,
    unavailable_reason: Option<&str>,
    claude: Option<crate::access::CapabilityEnforcement>,
    codex: Option<crate::access::CapabilityEnforcement>,
    theme: crate::theme::Theme,
) {
    let available = unavailable_reason.is_none();
    write!(
        output,
        "\n  {}  requested={}  available={}  Claude={}  Codex={}",
        theme.value(name),
        theme.success("yes"),
        if available {
            theme.success("yes")
        } else {
            theme.warning("no")
        },
        themed_enforcement(theme, claude),
        themed_enforcement(theme, codex)
    )
    .expect("writing to a String cannot fail");
    if let Some(reason) = unavailable_reason {
        write!(output, "\n    {}", theme.muted(reason)).expect("writing to a String cannot fail");
    }
}

const fn enforcement_label(
    enforcement: Option<crate::access::CapabilityEnforcement>,
) -> &'static str {
    match enforcement {
        Some(crate::access::CapabilityEnforcement::StrictlySelected) => "strictly-selected",
        Some(crate::access::CapabilityEnforcement::AdvisoryOnly) => "advisory-only",
        Some(crate::access::CapabilityEnforcement::Unavailable) | None => "unavailable",
    }
}

fn themed_enforcement(
    theme: crate::theme::Theme,
    enforcement: Option<crate::access::CapabilityEnforcement>,
) -> String {
    let label = enforcement_label(enforcement);
    match enforcement {
        Some(crate::access::CapabilityEnforcement::StrictlySelected) => theme.success(label),
        Some(crate::access::CapabilityEnforcement::AdvisoryOnly) => theme.warning(label),
        Some(crate::access::CapabilityEnforcement::Unavailable) | None => theme.error(label),
    }
}

/// Run `brain skills sync`. `root` (from `--root`) selects a sandbox.
pub fn run_sync(workspace: &crate::workspace::WorkspaceContext, root: Option<&Path>) -> Result<()> {
    let (layout, sources) = if let Some(r) = root {
        (
            Layout::under_root(r),
            Sources {
                extensions_dir: Some(r.join("extensions")),
                plugins_dir: Some(r.join("plugins")),
            },
        )
    } else {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("$HOME is not set"))?;
        (Layout::real(&home), super::real_sources(workspace))
    };
    let theme = crate::theme::Theme::active();
    eprintln!("{}", format_sync_plan(&layout, &sources, theme));
    crate::logging::log(format!(
        "skills sync built={} registry={} frontends={}",
        layout.built_dir.display(),
        layout.agents_dir.display(),
        layout.frontends.len()
    ));
    let report = install::sync(&layout, &sources)?;
    crate::logging::log(format!("skills sync installed={}", report.installed.len()));
    // A real (non-sandbox) sync is an authoritative render, so record the brain
    // version that produced it — this is what the startup auto-resync checks.
    // A `--root` sandbox run touches no live registry, so it leaves no stamp.
    if root.is_none() {
        super::record_synced_version(workspace, super::current_version());
    }
    println!(
        "{} {}",
        theme.success(&format!("synced {} skill(s):", report.installed.len())),
        theme.muted(&report.installed.join(", "))
    );
    Ok(())
}

#[must_use]
pub fn format_sync_plan(layout: &Layout, sources: &Sources, theme: crate::theme::Theme) -> String {
    let extensions = sources
        .extensions_dir
        .as_ref()
        .map_or_else(|| "none".to_owned(), |p| p.display().to_string());
    let plugins = sources
        .plugins_dir
        .as_ref()
        .map_or_else(|| "none".to_owned(), |p| p.display().to_string());
    format!(
        "{}\n  {} {}\n  {} {}\n  {} {}\n  {} {}\n  {} {}",
        theme.heading("Rendering and installing brain skills"),
        theme.muted("built:"),
        theme.value(&layout.built_dir.display().to_string()),
        theme.muted("registry:"),
        theme.value(&layout.agents_dir.display().to_string()),
        theme.muted("frontends:"),
        theme.value(&layout.frontends.len().to_string()),
        theme.muted("extensions:"),
        theme.value(&extensions),
        theme.muted("plugins:"),
        theme.value(&plugins),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_plan_names_skill_install_destinations() {
        let layout = Layout::under_root(Path::new("/tmp/brain-skills"));
        let sources = Sources {
            extensions_dir: Some(PathBuf::from("/tmp/brain-skills/extensions")),
            plugins_dir: Some(PathBuf::from("/tmp/brain-skills/plugins")),
        };

        let plan = format_sync_plan(&layout, &sources, crate::theme::Theme::dark(false));

        assert!(
            plan.contains("Rendering and installing brain skills"),
            "{plan}"
        );
        assert!(plan.contains("built: /tmp/brain-skills/built"), "{plan}");
        assert!(
            plan.contains("registry: /tmp/brain-skills/agents/skills"),
            "{plan}"
        );
        assert!(plan.contains("frontends: 4"), "{plan}");
        assert!(
            plan.contains("extensions: /tmp/brain-skills/extensions"),
            "{plan}"
        );
        assert!(
            plan.contains("plugins: /tmp/brain-skills/plugins"),
            "{plan}"
        );
    }
}
