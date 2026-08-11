//! Bare `brain receiver`: this machine's webhook URLs and every registered
//! workspace's receiver details.
//!
//! A machine hosts several workspaces and each configures its receiver
//! separately, so the question "what is my receiver set up as" is a machine-wide
//! one by default. `-w` narrows the workspace blocks to the one that was asked
//! about; the URL block above them is machine-wide either way, because there is
//! one URL per channel and a workspace's own number and address are what route
//! a message to it.

use anyhow::Result;

use crate::server::receiver::Channel;
use crate::theme::Theme;
use crate::workspace::{CommandContext, WorkspaceId, WorkspaceName, WorkspaceRecord};

use super::enablement::ReceiverStatus;
use super::identity;

/// Label column width for the listing, from its longest label.
const DETAIL_LABEL_WIDTH: usize = 10;

/// Label column width for `brain receiver status`, from its longest label.
const STATUS_LABEL_WIDTH: usize = 9;

/// What the machine-wide URL block calls this machine's configured origin.
const PUBLIC_URL_LABEL: &str = "Public URL";

/// Everything the listing reports about one workspace's receiver.
pub(crate) struct ReceiverDetails {
    pub(crate) workspace: String,
    pub(crate) enabled: bool,
    /// `None` when the live shared process could not be asked.
    pub(crate) live: Option<ReceiverStatus>,
    pub(crate) email: Option<String>,
    pub(crate) phone: Option<String>,
}

/// One workspace's row in the listing: its details, or why they are missing.
pub(crate) enum WorkspaceReport {
    Details(Box<ReceiverDetails>),
    Unavailable { workspace: String, reason: String },
}

/// One label/value row, padded to a fixed label column. Pure.
///
/// The value arrives already styled so a row can say "not set" in muted rather
/// than dress a missing value up as a real one.
fn row(label: &str, value: &str, width: usize, theme: Theme) -> String {
    // Pad before styling: color escapes have no display width, so a
    // format-width applied to a styled string misaligns the column.
    let label = format!("{label:width$}");
    format!("{} {value}", theme.muted(&label))
}

/// The intent-and-liveness rows shared by the listing and `receiver status`. Pure.
fn liveness_rows(status: ReceiverStatus, width: usize, theme: Theme) -> String {
    [
        (
            "Receiver",
            if status.enabled {
                "enabled"
            } else {
                "disabled"
            },
        ),
        ("TUI", if status.tui_live { "live" } else { "not live" }),
        (
            "Server",
            if status.server_running {
                "running"
            } else {
                "not running"
            },
        ),
        ("Accepting", if status.accepting { "yes" } else { "no" }),
    ]
    .into_iter()
    .map(|(label, value)| row(label, &theme.value(value), width, theme))
    .collect::<Vec<_>>()
    .join("\n")
}

/// One address row, or a muted `not set` when the channel has no address. Pure.
fn address_row(label: &str, value: Option<&str>, theme: Theme) -> String {
    let rendered = value.map_or_else(|| theme.muted("not set"), |value| theme.value(value));
    row(label, &rendered, DETAIL_LABEL_WIDTH, theme)
}

/// The four intent-and-liveness rows `brain receiver status` prints. Pure.
#[must_use]
pub(super) fn status_rows(status: ReceiverStatus, theme: Theme) -> String {
    liveness_rows(status, STATUS_LABEL_WIDTH, theme)
}

/// One workspace's receiver block. Pure.
#[must_use]
pub(crate) fn report_block(report: &WorkspaceReport, theme: Theme) -> String {
    match report {
        WorkspaceReport::Details(details) => details_block(details, theme),
        WorkspaceReport::Unavailable { workspace, reason } => {
            unavailable_block(workspace, reason, theme)
        }
    }
}

fn heading(workspace: &str, theme: Theme) -> String {
    theme.heading(&format!("Receiver details  {workspace}"))
}

/// One workspace's intent-and-liveness lines. Pure.
///
/// Saying "not running" when nobody answered would invent a fact, so an
/// unreachable shared process reports the persisted intent and no more.
fn liveness_block(details: &ReceiverDetails, theme: Theme) -> Vec<String> {
    let Some(status) = details.live else {
        return vec![
            row(
                "Receiver",
                &theme.value(if details.enabled {
                    "enabled"
                } else {
                    "disabled"
                }),
                DETAIL_LABEL_WIDTH,
                theme,
            ),
            row(
                "",
                &theme.muted("live state unavailable"),
                DETAIL_LABEL_WIDTH,
                theme,
            ),
        ];
    };
    vec![liveness_rows(status, DETAIL_LABEL_WIDTH, theme)]
}

fn details_block(details: &ReceiverDetails, theme: Theme) -> String {
    let mut body = liveness_block(details, theme);
    body.push(address_row(
        identity::address_label(Channel::Email),
        details.email.as_deref(),
        theme,
    ));
    body.push(address_row(
        identity::address_label(Channel::Sms),
        details.phone.as_deref(),
        theme,
    ));
    format!(
        "{}\n{}",
        heading(&details.workspace, theme),
        indent(&body.join("\n")),
    )
}

/// The machine's one webhook URL per channel, printed once above the workspace
/// blocks. Pure.
///
/// No URL names a workspace, so this belongs to the machine; the addresses in
/// each workspace block below are what route a message to that workspace.
#[must_use]
pub(crate) fn machine_block(public_url: Option<&str>, theme: Theme) -> String {
    let width = super::url::label_width(&super::url::ALL_CHANNELS).max(PUBLIC_URL_LABEL.len());
    // Pad before styling: color escapes have no display width, so a
    // format-width applied to a styled string misaligns the column.
    let label = format!("{PUBLIC_URL_LABEL:width$}");
    let mut body = vec![format!(
        "  {}  {}",
        theme.muted(&label),
        public_url.map_or_else(
            || theme.muted("not set"),
            |public_url| theme.value(public_url)
        ),
    )];
    // No origin means no webhook URL exists yet, so there is nothing to paste.
    match public_url {
        Some(public_url) => {
            body.push(super::url::webhook_rows_at(
                public_url,
                &super::url::ALL_CHANNELS,
                width,
                theme,
            ));
            body.push(format!("  {}", theme.muted(super::url::ROUTING_RULE)));
        }
        // The listing spans every workspace, so it points at the machine-wide
        // write rather than at a guided setup that would target only one.
        None => body.push(format!(
            "  {}",
            theme.muted(
                "Set the origin these webhook URLs are built from with `brain env set brain_receiver_public_url=https://<public-host>`."
            )
        )),
    }
    format!(
        "{}\n{}",
        theme.heading("Receiver webhook URLs"),
        body.join("\n"),
    )
}

fn unavailable_block(workspace: &str, reason: &str, theme: Theme) -> String {
    format!(
        "{}\n{}",
        heading(workspace, theme),
        indent(&theme.warning(&format!(
            "unavailable: {reason} (fix: brain workspace repair -w {workspace})"
        ))),
    )
}

/// Nest a block's every line under its workspace heading. Pure.
fn indent(block: &str) -> String {
    block
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The whole listing: this machine's webhook URLs, then one block per
/// workspace. Pure.
#[must_use]
pub(crate) fn listing(
    public_url: Option<&str>,
    reports: &[WorkspaceReport],
    theme: Theme,
) -> String {
    std::iter::once(machine_block(public_url, theme))
        .chain(reports.iter().map(|report| report_block(report, theme)))
        .map(|block| format!("{block}\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Bare `brain receiver`, for every registered workspace or just the selected one.
pub(super) fn run(context: &CommandContext, explicit_workspace: bool) -> Result<()> {
    let registry = crate::workspace::RegistryStore::load_readable(context.registry_store.path())?;
    let selected = context.workspace.id();
    let reports = registry
        .workspaces
        .iter()
        .filter(|(_, record)| !explicit_workspace || record.workspace_id == selected)
        .map(|(name, record)| {
            if record.workspace_id == selected {
                return workspace_report(name, record, Some(context));
            }
            workspace_report(
                name,
                record,
                crate::workspace::peer_context(name, record).as_ref(),
            )
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        listing(
            super::url::public_base_url(context).as_deref(),
            &reports,
            Theme::active(),
        )
    );
    Ok(())
}

fn workspace_report(
    name: &WorkspaceName,
    record: &WorkspaceRecord,
    context: Option<&CommandContext>,
) -> WorkspaceReport {
    let workspace = name.as_str().to_owned();
    let Some(context) = context else {
        return WorkspaceReport::Unavailable {
            workspace,
            reason: "workspace needs setup".to_owned(),
        };
    };
    WorkspaceReport::Details(Box::new(ReceiverDetails {
        workspace,
        enabled: record.receiver_enabled,
        live: live_status(record.receiver_enabled, record.workspace_id),
        email: identity::address(context, Channel::Email),
        phone: identity::address(context, Channel::Sms),
    }))
}

/// One workspace's live receiver state, or `None` when nobody could answer.
fn live_status(enabled: bool, workspace_id: WorkspaceId) -> Option<ReceiverStatus> {
    let live = crate::server::lifecycle::ServerClient::default()
        .workspace_status(workspace_id)
        .ok()?;
    Some(super::enablement::receiver_status(
        enabled,
        live.is_some(),
        live.and_then(|status| status.receiver_enabled),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        ReceiverDetails, ReceiverStatus, WorkspaceReport, listing, machine_block, report_block,
    };
    use crate::theme::Theme;

    const PUBLIC_URL: &str = "https://brain.example.test";

    fn configured() -> ReceiverDetails {
        ReceiverDetails {
            workspace: "family".to_owned(),
            enabled: true,
            live: Some(ReceiverStatus {
                enabled: true,
                tui_live: true,
                server_running: true,
                accepting: true,
            }),
            email: Some("brain@example.test".to_owned()),
            phone: Some("+12125550100".to_owned()),
        }
    }

    fn block_of(details: ReceiverDetails) -> String {
        report_block(
            &WorkspaceReport::Details(Box::new(details)),
            Theme::dark(false),
        )
    }

    #[test]
    fn a_configured_workspace_reports_the_addresses_that_route_to_it() {
        let block = block_of(configured());

        assert!(block.starts_with("Receiver details  family"), "{block}");
        assert!(block.contains("Receiver"), "{block}");
        assert!(block.contains("Accepting"), "{block}");
        // The whole point of the listing: the addresses inbound senders use,
        // which are also what selects this workspace over any other.
        assert!(block.contains("Email"), "{block}");
        assert!(block.contains("brain@example.test"), "{block}");
        assert!(block.contains("Phone"), "{block}");
        assert!(block.contains("+12125550100"), "{block}");
        // The URLs are machine-wide, so no workspace block repeats them.
        assert!(!block.contains("http"), "{block}");
    }

    #[test]
    fn an_unconfigured_channel_reads_as_not_set_rather_than_a_blank_column() {
        let block = block_of(ReceiverDetails {
            enabled: false,
            live: Some(ReceiverStatus {
                enabled: false,
                tui_live: false,
                server_running: false,
                accepting: false,
            }),
            email: None,
            phone: None,
            ..configured()
        });

        assert_eq!(block.matches("not set").count(), 2, "{block}");
        assert!(block.contains("disabled"), "{block}");
    }

    #[test]
    fn the_machine_block_pairs_one_origin_with_one_url_per_channel() {
        let block = machine_block(Some(PUBLIC_URL), Theme::dark(false));

        assert!(block.starts_with("Receiver webhook URLs"), "{block}");
        assert!(block.contains("Public URL"), "{block}");
        assert!(block.contains("https://brain.example.test/sms"), "{block}");
        assert!(
            block.contains("https://brain.example.test/email"),
            "{block}"
        );
        assert!(!block.contains("/w/"), "{block}");
        // Why one URL can serve every workspace here.
        assert!(
            block.contains("routes each message by the number"),
            "{block}"
        );
    }

    #[test]
    fn without_an_origin_the_machine_block_says_so_and_invents_no_url() {
        let block = machine_block(None, Theme::dark(false));

        assert!(block.contains("not set"), "{block}");
        assert!(!block.contains("/sms"), "{block}");
        assert!(
            block.contains("brain env set brain_receiver_public_url="),
            "{block}"
        );
    }

    #[test]
    fn an_unreachable_shared_process_is_said_plainly_and_never_faked_as_stopped() {
        let block = block_of(ReceiverDetails {
            live: None,
            ..configured()
        });

        assert!(block.contains("enabled"), "{block}");
        assert!(block.contains("live state unavailable"), "{block}");
        assert!(!block.contains("Accepting"), "{block}");
    }

    #[test]
    fn an_unreadable_workspace_names_itself_and_the_repair_command() {
        let block = report_block(
            &WorkspaceReport::Unavailable {
                workspace: "family".to_owned(),
                reason: "workspace needs setup".to_owned(),
            },
            Theme::dark(false),
        );

        assert!(block.contains("family"), "{block}");
        assert!(block.contains("workspace needs setup"), "{block}");
        assert!(
            block.contains("brain workspace repair -w family"),
            "{block}"
        );
    }

    #[test]
    fn the_listing_leads_with_the_machine_urls_then_one_block_per_workspace() {
        let rendered = listing(
            Some(PUBLIC_URL),
            &[
                WorkspaceReport::Details(Box::new(configured())),
                WorkspaceReport::Unavailable {
                    workspace: "personal".to_owned(),
                    reason: "workspace needs setup".to_owned(),
                },
            ],
            Theme::dark(false),
        );

        // The URLs come once, before the workspaces they can all reach.
        assert!(rendered.starts_with("Receiver webhook URLs"), "{rendered}");
        assert_eq!(rendered.matches("/sms").count(), 1, "{rendered}");
        assert!(rendered.contains("Receiver details  family"), "{rendered}");
        assert!(rendered.contains("personal"), "{rendered}");
        assert!(rendered.contains("\n\n"), "{rendered}");
        assert!(rendered.ends_with('\n'), "{rendered}");
    }
}
