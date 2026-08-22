//! Tests for link extraction/classification and the link-picker modal.

use crate::tui::*;
use anyhow::Result;
use std::sync::Mutex;

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
