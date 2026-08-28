use super::{
    ReceiverDetails, ReceiverStatus, WorkspaceReport, listing, machine_block, report_block,
};
use crate::state::ReceiverDeliveryCounts;
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
fn delivery_status_rows_are_themed_stable_counts_without_private_content() {
    let rendered = super::delivery_rows(
        ReceiverDeliveryCounts::new(1, 2, 3, 4, 5, 6),
        Theme::dark(true),
    );

    for phase in [
        "answer-ready",
        "delivering",
        "retrying",
        "ambiguous",
        "failed",
        "done",
    ] {
        assert!(rendered.contains(phase), "missing {phase}: {rendered}");
    }
    for count in 1..=6 {
        assert!(
            rendered.contains(&count.to_string()),
            "missing count {count}"
        );
    }
    assert!(
        rendered.contains("\u{1b}["),
        "delivery rows were not themed"
    );
    for private in ["private-sender", "private-answer", "credential-secret"] {
        assert!(!rendered.contains(private));
    }
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
