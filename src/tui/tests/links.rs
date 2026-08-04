//! Tests for link extraction/classification and the link-picker modal.

use crate::tui::*;
use anyhow::Result;
use std::sync::Mutex;

// The `tui` module is bin-only, so its tests can't reach the library's
// `#[cfg(test)]` `task::test_task` helper. Keep a local minimal builder.
fn test_task(id: &str, status: &str) -> Task {
    Task {
        task_uuid: None,
        id: id.to_owned(),
        name: format!("test task {id}"),
        types: Vec::new(),
        status: status.to_owned(),
        priority: "p2".to_owned(),
        due_date: None,
        hard_deadline: false,
        start_date: None,
        assigned_to: String::new(),
        notes: String::new(),
        project: String::new(),
        energy: String::new(),
        context: String::new(),
        estimated_duration: None,
        defer_count: 0,
        last_touched: None,
        see_also: String::new(),
        blocked_by: Vec::new(),
        completed_date: None,
        linear_issue: String::new(),
        system_key: String::new(),
    }
}

/// we can exercise the failure branch too.
struct RecordingOpener {
    opened: Mutex<Vec<String>>,
    fail: bool,
}

impl RecordingOpener {
    fn new(fail: bool) -> Self {
        Self {
            opened: Mutex::new(Vec::new()),
            fail,
        }
    }
}

impl ShellRunner for RecordingOpener {
    fn run(&self) -> Result<()> {
        Ok(())
    }
    fn open(&self, url: &str) -> Result<()> {
        self.opened.lock().unwrap().push(url.to_owned());
        if self.fail {
            anyhow::bail!("boom");
        }
        Ok(())
    }
}

#[test]
fn open_url_opens_and_reports_success() {
    let opener = RecordingOpener::new(false);
    let flash = open_url(&opener, "https://example.com/x");
    assert_eq!(
        opener.opened.lock().unwrap().as_slice(),
        ["https://example.com/x"],
    );
    assert!(matches!(flash, FlashKind::Info(_)));
}

#[test]
fn open_url_surfaces_error_flash_on_failure() {
    let opener = RecordingOpener::new(true);
    let flash = open_url(&opener, "https://example.com/x");
    // It still issued the open call, but reports the failure.
    assert_eq!(opener.opened.lock().unwrap().len(), 1);
    assert!(matches!(flash, FlashKind::Error(_)));
}

// --- extract_urls ---

#[test]
fn extract_urls_finds_in_order_and_dedups() {
    let text = "see https://a.com and http://b.org then https://a.com again";
    assert_eq!(
        extract_urls(text),
        vec!["https://a.com".to_owned(), "http://b.org".to_owned()]
    );
}

#[test]
fn extract_urls_trims_trailing_punctuation() {
    assert_eq!(
        extract_urls("docs at https://a.com/page."),
        vec!["https://a.com/page".to_owned()]
    );
    // Parenthetical: the closing paren is not part of the URL.
    assert_eq!(
        extract_urls("(https://a.com/p)"),
        vec!["https://a.com/p".to_owned()]
    );
}

#[test]
fn extract_urls_empty_when_no_urls() {
    assert!(extract_urls("just some plain notes, no links").is_empty());
}

// --- task_links ---

const LINEAR_BASE: &str = "https://linear.app/acme/issue/";

#[test]
fn task_links_linear_first_then_notes_urls() {
    let mut t = test_task("T9", "not_started");
    t.linear_issue = "AVA-123".to_owned();
    t.notes = "spec: https://spec.example.com and https://design.example.com".to_owned();
    let links = task_links(&t, LINEAR_BASE);
    let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
    assert_eq!(
        urls,
        vec![
            "https://linear.app/acme/issue/AVA-123",
            "https://spec.example.com",
            "https://design.example.com",
        ]
    );
    assert_eq!(links[0].label, "Linear AVA-123");
}

#[test]
fn task_links_linear_only_when_no_notes_urls() {
    let mut t = test_task("T9", "not_started");
    t.linear_issue = "AVA-123".to_owned();
    let links = task_links(&t, LINEAR_BASE);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://linear.app/acme/issue/AVA-123");
}

#[test]
fn task_links_notes_urls_only_when_no_linear() {
    let mut t = test_task("T9", "not_started"); // no linear_issue
    t.notes = "ref https://only.example.com".to_owned();
    let links = task_links(&t, LINEAR_BASE);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://only.example.com");
    // No linear → the single URL's label is the URL itself.
    assert_eq!(links[0].label, "https://only.example.com");
}

#[test]
fn task_links_picks_up_see_also_url_before_notes() {
    // The real T90 shape: the only link lives in `see_also`, not `notes`.
    let mut t = test_task("T90", "not_started"); // no linear_issue
    t.see_also = "https://www.notion.so/workspace/Call-abc".to_owned();
    t.notes = "later ref https://docs.example.com".to_owned();
    let links = task_links(&t, LINEAR_BASE);
    let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
    assert_eq!(
        urls,
        vec![
            "https://www.notion.so/workspace/Call-abc",
            "https://docs.example.com",
        ]
    );
}

#[test]
fn task_links_empty_when_no_linear_and_no_urls() {
    let t = test_task("T9", "not_started");
    assert!(task_links(&t, LINEAR_BASE).is_empty());
}

#[test]
fn task_links_dedups_linear_url_appearing_in_notes() {
    let mut t = test_task("T9", "not_started");
    t.linear_issue = "AVA-9".to_owned();
    // Notes repeat the Linear URL — it must not be listed twice.
    t.notes = "tracking https://linear.app/acme/issue/AVA-9 here".to_owned();
    let links = task_links(&t, LINEAR_BASE);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].label, "Linear AVA-9");
}

// --- classify_links ---

#[test]
fn classify_links_covers_each_kind() {
    // None: no linear, no notes URLs.
    let mut t = test_task("T9", "not_started");
    assert_eq!(
        classify_links(&t, &task_links(&t, LINEAR_BASE)),
        LinkKind::None
    );

    // SingleLinear: a lone Linear issue.
    t.linear_issue = "AVA-1".to_owned();
    assert_eq!(
        classify_links(&t, &task_links(&t, LINEAR_BASE)),
        LinkKind::SingleLinear
    );

    // SingleNotes: one notes URL, no Linear.
    let mut n = test_task("T90", "not_started");
    n.notes = "https://notion.so/page".to_owned();
    assert_eq!(
        classify_links(&n, &task_links(&n, LINEAR_BASE)),
        LinkKind::SingleNotes
    );

    // Multiple: Linear + a notes URL.
    let mut m = test_task("T5", "not_started");
    m.linear_issue = "AVA-2".to_owned();
    m.notes = "https://docs.example.com".to_owned();
    assert_eq!(
        classify_links(&m, &task_links(&m, LINEAR_BASE)),
        LinkKind::Multiple
    );
}

// --- LinkPickerState ---

fn picker_with(n: usize) -> LinkPickerState {
    let links = (0..n)
        .map(|i| Link {
            label: format!("L{i}"),
            url: format!("https://e{i}.com"),
        })
        .collect();
    LinkPickerState::new("T1".to_owned(), links)
}

#[test]
fn link_picker_navigation_clamps_at_both_ends() {
    let mut p = picker_with(3);
    assert_eq!(p.selected(), 0);
    p.move_up(); // clamped at top
    assert_eq!(p.selected(), 0);
    p.move_down();
    p.move_down();
    p.move_down(); // clamped at bottom (2)
    assert_eq!(p.selected(), 2);
    assert_eq!(p.selected_url(), Some("https://e2.com"));
}

#[test]
fn link_picker_select_number_jumps_one_based_and_rejects_out_of_range() {
    let mut p = picker_with(3);
    assert!(p.select_number(2));
    assert_eq!(p.selected(), 1);
    assert!(!p.select_number(0)); // no zero row
    assert!(!p.select_number(4)); // past the end
    assert_eq!(p.selected(), 1); // unchanged by the rejects
}
