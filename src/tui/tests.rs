//! Unit tests for the tui module. Items under test are re-exported into
//! the `tui` root, so `use super::*` reaches them all.

use super::*;
use crate::session::shell_quote;
use crate::tasks::task::Task;
use crate::tasks::view::View;
use anyhow::Result;
use crossterm::event::KeyCode;
use std::sync::Mutex;

    // The `tui` module is bin-only, so its tests can't reach the library's
    // `#[cfg(test)]` `task::test_task` helper. Keep a local minimal builder.
    fn test_task(id: &str, status: &str) -> Task {
        Task {
            id: id.to_owned(),
            name: format!("test task {id}"),
            types: Vec::new(),
            status: status.to_owned(),
            priority: "p2".to_owned(),
            due_date: None,
            hard_deadline: false,
            start_date: None,
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

    const LINEAR_BASE: &str = "https://linear.app/avandar/issue/";

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
                "https://linear.app/avandar/issue/AVA-123",
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
        assert_eq!(links[0].url, "https://linear.app/avandar/issue/AVA-123");
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
        t.see_also = "https://www.notion.so/pablosarmiento/Call-abc".to_owned();
        t.notes = "later ref https://docs.example.com".to_owned();
        let links = task_links(&t, LINEAR_BASE);
        let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://www.notion.so/pablosarmiento/Call-abc",
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
        t.notes = "tracking https://linear.app/avandar/issue/AVA-9 here".to_owned();
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

    // --- enter_inserts_newline: multiline compose in the brain-input modal ---

    #[test]
    fn bare_enter_submits_not_newline() {
        assert!(!enter_inserts_newline(false));
    }

    #[test]
    fn alt_enter_inserts_a_newline() {
        // Alt+Enter is the reliable newline binding (distinct Meta sequence
        // on every terminal); a bare Enter still submits.
        assert!(enter_inserts_newline(true));
    }

    // --- ctrl_opens_brain vs ctrl_messages_brain_about_task: Shift is what
    //     splits Ctrl+M (panel) from Ctrl+Shift+M (task-scoped message) ---

    #[test]
    fn ctrl_m_without_shift_opens_brain() {
        assert!(ctrl_opens_brain(KeyCode::Char('m'), true, false));
        // Kitty may report the shifted glyph; the chord still resolves.
        assert!(ctrl_opens_brain(KeyCode::Char('M'), true, false));
    }

    #[test]
    fn ctrl_shift_m_does_not_open_brain() {
        // Shift held → that's the task-scoped message, not the panel toggle.
        assert!(!ctrl_opens_brain(KeyCode::Char('m'), true, true));
        assert!(!ctrl_opens_brain(KeyCode::Char('M'), true, true));
    }

    #[test]
    fn bare_m_does_not_open_brain() {
        // No Ctrl → that's the "jump to MIT view" letter, not a brain chord.
        assert!(!ctrl_opens_brain(KeyCode::Char('m'), false, false));
    }

    #[test]
    fn ctrl_shift_m_messages_brain_about_task() {
        assert!(ctrl_messages_brain_about_task(KeyCode::Char('m'), true, true));
        assert!(ctrl_messages_brain_about_task(KeyCode::Char('M'), true, true));
    }

    #[test]
    fn ctrl_m_without_shift_does_not_message_about_task() {
        assert!(!ctrl_messages_brain_about_task(KeyCode::Char('m'), true, false));
    }

    #[test]
    fn shift_m_without_ctrl_does_not_message_about_task() {
        // Plain Shift+M (a capital M keystroke) is not a chord.
        assert!(!ctrl_messages_brain_about_task(KeyCode::Char('M'), false, true));
    }

    // --- visual_row_offsets / visual_range: logical-line ↔ wrapped-row
    //     translation so selection + scroll survive wrapped notes ---

    #[test]
    fn offsets_are_a_prefix_sum_of_wrapped_heights() {
        // Three single-row lines → rows 0,1,2 and a total of 3.
        assert_eq!(visual_row_offsets(&[1, 1, 1]), vec![0, 1, 2, 3]);
    }

    #[test]
    fn offsets_account_for_a_wrapped_line() {
        // The middle line wraps to 3 visual rows, so every line after it is
        // pushed down by 2 — exactly the drift that broke the highlight.
        assert_eq!(visual_row_offsets(&[1, 3, 1]), vec![0, 1, 4, 5]);
    }

    #[test]
    fn empty_body_has_a_single_zero_offset() {
        assert_eq!(visual_row_offsets(&[]), vec![0]);
    }

    #[test]
    fn visual_range_maps_a_task_past_a_wrapped_line() {
        // offsets for heights [1,3,1]; task at logical line 1 occupies the
        // 3 wrapped rows 1..4, not the logical 1..2.
        let offsets = visual_row_offsets(&[1, 3, 1]);
        assert_eq!(visual_range(&offsets, 1..2), 1..4);
        assert_eq!(visual_range(&offsets, 0..1), 0..1);
        assert_eq!(visual_range(&offsets, 2..3), 4..5);
    }

    #[test]
    fn visual_range_clamps_out_of_bounds_indices() {
        let offsets = visual_row_offsets(&[1, 1]); // [0,1,2]
        assert_eq!(visual_range(&offsets, 5..9), 2..2);
        assert_eq!(visual_range(&[], 0..3), 0..0);
    }

    // --- modal_input_target: the help modal preempts; the rest keep order ---

    fn modals(help: bool, palette: bool, brain_input: bool, confirm: bool) -> ActiveModals {
        ActiveModals {
            help,
            palette,
            brain_input,
            confirm,
            link_picker: false,
        }
    }

    fn modals_with_picker(link_picker: bool) -> ActiveModals {
        ActiveModals {
            help: false,
            palette: false,
            brain_input: false,
            confirm: false,
            link_picker,
        }
    }

    #[test]
    fn help_modal_preempts_every_other_modal() {
        // Help is captive: while it's up it grabs input over any other modal
        // that happened to be open.
        assert_eq!(
            modal_input_target(modals(true, true, true, true)),
            ModalInput::Help
        );
        assert_eq!(
            modal_input_target(modals(true, false, false, false)),
            ModalInput::Help
        );
    }

    #[test]
    fn without_help_other_modals_keep_their_order() {
        assert_eq!(
            modal_input_target(modals(false, true, true, true)),
            ModalInput::Palette
        );
        assert_eq!(
            modal_input_target(modals(false, false, true, true)),
            ModalInput::BrainInput
        );
        assert_eq!(
            modal_input_target(modals(false, false, false, true)),
            ModalInput::Confirm
        );
    }

    #[test]
    fn no_modal_routes_to_panels() {
        assert_eq!(
            modal_input_target(modals(false, false, false, false)),
            ModalInput::Panels
        );
    }

    #[test]
    fn link_picker_routes_when_no_higher_modal_is_open() {
        assert_eq!(
            modal_input_target(modals_with_picker(true)),
            ModalInput::LinkPicker
        );
        // A higher-precedence modal (e.g. confirm) still wins over the picker.
        assert_eq!(
            modal_input_target(modals(false, false, false, true)),
            ModalInput::Confirm
        );
    }

    // --- PaletteState: notes toggle ---

    fn has_toggle(state: &PaletteState) -> bool {
        state
            .visible()
            .iter()
            .any(|c| matches!(c.action, PaletteAction::ToggleNotes))
    }

    fn toggle_label(state: &PaletteState) -> Option<String> {
        state
            .visible()
            .iter()
            .find(|c| matches!(c.action, PaletteAction::ToggleNotes))
            .map(|c| state.label_for(c))
    }

    #[test]
    fn notes_toggle_hidden_when_task_has_no_notes() {
        let state = PaletteState::new_task_actions("T1".into(), "task".into(), false, false, false, LinkKind::None);
        assert!(!has_toggle(&state));
    }

    #[test]
    fn notes_toggle_shown_and_reads_expand_when_collapsed() {
        let state = PaletteState::new_task_actions("T1".into(), "task".into(), false, true, false, LinkKind::None);
        assert!(has_toggle(&state));
        assert_eq!(toggle_label(&state).as_deref(), Some("Expand notes"));
    }

    #[test]
    fn notes_toggle_reads_collapse_when_expanded() {
        let state = PaletteState::new_task_actions("T1".into(), "task".into(), false, true, true, LinkKind::None);
        assert_eq!(toggle_label(&state).as_deref(), Some("Collapse notes"));
    }

    #[test]
    fn notes_toggle_available_for_habits_with_notes() {
        // Habits can carry notes too; the toggle is `works_on_habits`.
        let state = PaletteState::new_task_actions("H1".into(), "habit".into(), true, true, false, LinkKind::None);
        assert!(has_toggle(&state));
    }

    #[test]
    fn notes_toggle_in_global_palette_names_the_task() {
        // In the global command palette the toggle follows the task-ID convention
        // of the other task-specific commands ("Expand T123 notes").
        let state = PaletteState::new(Some("T123".into()), false, true, false, LinkKind::None, false);
        assert_eq!(toggle_label(&state).as_deref(), Some("Expand T123 notes"));
    }

    #[test]
    fn notes_toggle_in_global_palette_reads_collapse_when_expanded() {
        let state = PaletteState::new(Some("T123".into()), false, true, true, LinkKind::None, false);
        assert_eq!(
            toggle_label(&state).as_deref(),
            Some("Collapse T123 notes")
        );
    }

    // --- PaletteState: "open link" gating + per-kind label ---

    fn has_open_links(state: &PaletteState) -> bool {
        state
            .visible()
            .iter()
            .any(|c| matches!(c.action, PaletteAction::OpenLinks))
    }

    fn open_links_label(state: &PaletteState) -> Option<String> {
        state
            .visible()
            .iter()
            .find(|c| matches!(c.action, PaletteAction::OpenLinks))
            .map(|c| state.label_for(c))
    }

    #[test]
    fn open_links_hidden_when_task_has_no_links() {
        let state = PaletteState::new_task_actions(
            "T1".into(),
            "task".into(),
            false,
            false,
            false,
            LinkKind::None,
        );
        assert!(!has_open_links(&state));
    }

    #[test]
    fn open_links_single_linear_label() {
        // Actions modal (no id in the label) and global palette (named).
        let actions = PaletteState::new_task_actions(
            "T1".into(),
            "task".into(),
            false,
            false,
            false,
            LinkKind::SingleLinear,
        );
        assert!(has_open_links(&actions));
        assert_eq!(open_links_label(&actions).as_deref(), Some("Open Linear link"));

        let global = PaletteState::new(Some("T123".into()), false, false, false, LinkKind::SingleLinear, false);
        assert_eq!(
            open_links_label(&global).as_deref(),
            Some("Open T123 Linear link")
        );
    }

    #[test]
    fn open_links_single_notes_label() {
        let actions = PaletteState::new_task_actions(
            "T1".into(),
            "task".into(),
            false,
            false,
            false,
            LinkKind::SingleNotes,
        );
        assert!(has_open_links(&actions));
        assert_eq!(
            open_links_label(&actions).as_deref(),
            Some("Open link from note")
        );

        let global = PaletteState::new(Some("T90".into()), false, false, false, LinkKind::SingleNotes, false);
        assert_eq!(
            open_links_label(&global).as_deref(),
            Some("Open link from T90's note")
        );
    }

    #[test]
    fn open_links_multiple_label() {
        let actions = PaletteState::new_task_actions(
            "T1".into(),
            "task".into(),
            false,
            false,
            false,
            LinkKind::Multiple,
        );
        assert!(has_open_links(&actions));
        assert_eq!(open_links_label(&actions).as_deref(), Some("Open attached link"));

        let global = PaletteState::new(Some("T123".into()), false, false, false, LinkKind::Multiple, false);
        assert_eq!(
            open_links_label(&global).as_deref(),
            Some("Open link attached to T123")
        );
    }

    #[test]
    fn open_links_advertises_its_ctrl_o_shortcut() {
        // The `[^O]` hint renders next to the label in both modals, mirroring
        // the other directly-bound actions (^D, ^N, …).
        assert_eq!(shortcut_for(PaletteAction::OpenLinks), Some("^O"));
    }

    #[test]
    fn open_links_shown_for_habit_with_notes_url() {
        // A habit has no Linear issue but can carry a notes URL; the command
        // is offered (works_on_habits) and gated only on having ≥ 1 link.
        let with_link = PaletteState::new_task_actions(
            "H1".into(),
            "habit".into(),
            true,
            false,
            false,
            LinkKind::SingleNotes,
        );
        assert!(has_open_links(&with_link));

        let no_link = PaletteState::new_task_actions(
            "H1".into(),
            "habit".into(),
            true,
            false,
            false,
            LinkKind::None,
        );
        assert!(!has_open_links(&no_link));
    }

    fn action_order(state: &PaletteState) -> Vec<PaletteAction> {
        state.visible().iter().map(|c| c.action).collect()
    }

    #[test]
    fn full_palette_lists_actions_in_canonical_order() {
        // Task with notes selected: start → complete → message-about →
        // message-global → notes → remove → defer group → other globals.
        let state = PaletteState::new(Some("T1".into()), false, true, false, LinkKind::None, false);
        assert_eq!(
            action_order(&state),
            vec![
                PaletteAction::StartTask,
                PaletteAction::MarkTaskComplete,
                PaletteAction::MessageBrainAboutTask,
                PaletteAction::SendBrainMessage,
                PaletteAction::ToggleNotes,
                PaletteAction::RemoveTask,
                PaletteAction::DeferTask(1),
                PaletteAction::DeferTask(7),
                PaletteAction::DeferTask(14),
                PaletteAction::OpenHabitsInBrowser,
                PaletteAction::OpenAgenda,
            ]
        );
    }

    #[test]
    fn task_actions_modal_palette_keeps_order_minus_globals() {
        // Same relative order, with the global commands filtered out.
        let state = PaletteState::new_task_actions("T1".into(), "task".into(), false, true, false, LinkKind::None);
        assert_eq!(
            action_order(&state),
            vec![
                PaletteAction::StartTask,
                PaletteAction::MarkTaskComplete,
                PaletteAction::MessageBrainAboutTask,
                PaletteAction::ToggleNotes,
                PaletteAction::RemoveTask,
                PaletteAction::DeferTask(1),
                PaletteAction::DeferTask(7),
                PaletteAction::DeferTask(14),
            ]
        );
    }

    // --- PaletteState: numbered rows (brain-menu parity) ---

    #[test]
    fn palette_rows_are_numbered_from_one_in_canonical_order() {
        // Numbers are the 1-based position in the scope-visible list, stable
        // regardless of the text filter — so the digit a user types always
        // points at the same command.
        let state = PaletteState::new(Some("T1".into()), false, true, false, LinkKind::None, false);
        let cmds = state.scoped();
        assert_eq!(state.number_for(cmds[0]), 1);
        assert_eq!(state.number_for(cmds[1]), 2);
        assert_eq!(state.number_for(cmds.last().unwrap()), cmds.len());
    }

    #[test]
    fn typing_a_row_number_filters_to_that_numbered_row() {
        // "2." prefixes the second command, so a query of "2" keeps it.
        let mut state = PaletteState::new(Some("T1".into()), false, true, false, LinkKind::None, false);
        let second = state.scoped()[1];
        state.append('2');
        let hits = state.visible();
        assert!(
            hits.iter().any(|c| c.action == second.action),
            "typing the row number should surface that numbered command"
        );
    }

    // --- wrap_input (brain-input soft wrapping) ---

    #[test]
    fn wrap_input_empty_is_one_blank_row() {
        assert_eq!(wrap_input("", 10), vec![String::new()]);
    }

    #[test]
    fn wrap_input_short_text_is_one_row() {
        assert_eq!(wrap_input("hello", 10), vec!["hello".to_owned()]);
    }

    #[test]
    fn wrap_input_breaks_on_word_boundary() {
        // "hello world" at width 8 breaks after "hello " (the space stays
        // on the first row), not mid-word.
        assert_eq!(
            wrap_input("hello world", 8),
            vec!["hello ".to_owned(), "world".to_owned()]
        );
    }

    #[test]
    fn wrap_input_hard_splits_overlong_word() {
        // No break opportunity within the window → hard break at width.
        assert_eq!(
            wrap_input("abcdefghij", 4),
            vec!["abcd".to_owned(), "efgh".to_owned(), "ij".to_owned()]
        );
    }

    #[test]
    fn wrap_input_honors_explicit_newlines() {
        assert_eq!(
            wrap_input("a\nb", 10),
            vec!["a".to_owned(), "b".to_owned()]
        );
    }

    #[test]
    fn wrap_input_trailing_newline_yields_trailing_blank_row() {
        // A trailing newline leaves an empty last row so the cursor lands
        // at the start of a fresh line.
        assert_eq!(
            wrap_input("hi\n", 10),
            vec!["hi".to_owned(), String::new()]
        );
    }

    #[test]
    fn wrap_input_preserves_every_character() {
        let text = "the quick brown fox jumps over the lazy dog";
        let joined: String = wrap_input(text, 11).concat();
        assert_eq!(joined, text);
    }

    // --- accumulate_count (vim-style count prefix) ---

    #[test]
    fn first_digit_starts_a_count() {
        assert_eq!(accumulate_count(None, 3), Some(3));
    }

    #[test]
    fn subsequent_digits_shift_and_append() {
        // Typing `1` then `2` then `0` builds 120 (e.g. 120j).
        let c = accumulate_count(None, 1);
        let c = accumulate_count(c, 2);
        assert_eq!(accumulate_count(c, 0), Some(120));
    }

    #[test]
    fn leading_zero_is_not_a_count() {
        // A bare `0` doesn't start a count, leaving the key free.
        assert_eq!(accumulate_count(None, 0), None);
    }

    #[test]
    fn zero_extends_an_in_progress_count() {
        // But `0` after a non-zero digit is a normal digit: `1` then `0`.
        assert_eq!(accumulate_count(Some(1), 0), Some(10));
    }

    #[test]
    fn count_is_capped_at_max() {
        // Runaway digit entry saturates rather than overflowing.
        let mut c = Some(MAX_COUNT);
        c = accumulate_count(c, 9);
        assert_eq!(c, Some(MAX_COUNT));
    }

    #[test]
    fn digits_and_motions_preserve_a_pending_count() {
        for c in ['0', '5', '9', 'j', 'k'] {
            assert!(is_count_relevant_key(KeyCode::Char(c), false));
        }
        assert!(is_count_relevant_key(KeyCode::Up, false));
        assert!(is_count_relevant_key(KeyCode::Down, false));
    }

    #[test]
    fn other_keys_clear_a_pending_count() {
        // Non-motion normal keys, and any ctrl-modified key, are not
        // count-relevant — they clear the prefix.
        assert!(!is_count_relevant_key(KeyCode::Char('g'), false));
        assert!(!is_count_relevant_key(KeyCode::Char('d'), false));
        assert!(!is_count_relevant_key(KeyCode::Char('l'), false));
        assert!(!is_count_relevant_key(KeyCode::Enter, false));
        assert!(!is_count_relevant_key(KeyCode::Char('/'), false));
        // Ctrl chords never participate, even on otherwise-relevant keys.
        assert!(!is_count_relevant_key(KeyCode::Char('j'), true));
        assert!(!is_count_relevant_key(KeyCode::Char('5'), true));
    }

    // --- h_collapses_notes ---

    #[test]
    fn h_collapses_when_highlighted_entry_has_expanded_notes() {
        // The motivating case: notes are expanded on the highlighted
        // entry, so `h` must collapse them rather than jump to habits.
        assert!(h_collapses_notes(true, true));
    }

    #[test]
    fn h_switches_to_habits_when_notes_collapsed() {
        // Entry has notes but they're collapsed — `h` is the habits
        // shortcut as usual.
        assert!(!h_collapses_notes(true, false));
    }

    #[test]
    fn h_switches_to_habits_when_entry_has_no_notes() {
        // No notes to collapse (even if `full_notes` reports "expanded"),
        // so `h` stays the habits shortcut and never dead-ends.
        assert!(!h_collapses_notes(false, true));
        assert!(!h_collapses_notes(false, false));
    }

    // --- search_delegates_ctrl_chord ---

    #[test]
    fn ctrl_enter_delegates_to_normal_in_search_mode() {
        // The motivating case: Ctrl+Enter opens the task actions modal on
        // the highlighted task even while the user is typing in the search
        // input (bare Enter exits search, so it can't do double duty).
        assert!(search_delegates_ctrl_chord(KeyCode::Enter, true));
    }

    #[test]
    fn ctrl_d_delegates_to_normal_in_search_mode() {
        // Ctrl+D ("done") must mark-complete the highlighted task from
        // inside `/` without first exiting search.
        assert!(search_delegates_ctrl_chord(KeyCode::Char('d'), true));
    }

    #[test]
    fn bare_enter_stays_in_search_mode() {
        // Enter without ctrl is search-specific: exit-search-keep-filter.
        assert!(!search_delegates_ctrl_chord(KeyCode::Enter, false));
    }

    #[test]
    fn ctrl_c_keeps_search_specific_handling() {
        // Ctrl+C is handled search-specifically (it exits `/` instead of
        // quitting the shell), so it must not be bounced to normal-mode.
        assert!(!search_delegates_ctrl_chord(KeyCode::Char('c'), true));
    }

    // --- search_key_abandons_filter ---

    #[test]
    fn ctrl_c_in_search_abandons_filter_not_quits() {
        // The motivating bug: Ctrl+C while typing in `/` should leave search
        // mode (clearing the filter), exactly like Esc — never quit the shell.
        assert!(search_key_abandons_filter(KeyCode::Char('c'), true));
    }

    #[test]
    fn esc_in_search_abandons_filter() {
        // Esc has always exited `/` and cleared the filter.
        assert!(search_key_abandons_filter(KeyCode::Esc, false));
    }

    #[test]
    fn bare_c_in_search_is_text_input() {
        // A bare `c` is query text, not an abandon-search chord.
        assert!(!search_key_abandons_filter(KeyCode::Char('c'), false));
    }

    #[test]
    fn ctrl_u_in_search_does_not_abandon_filter() {
        // Ctrl+U clears the query but stays in search mode — it is not an
        // abandon-and-exit chord.
        assert!(!search_key_abandons_filter(KeyCode::Char('u'), true));
    }

    #[test]
    fn ctrl_u_keeps_search_specific_handling() {
        // Ctrl+U clears the query in search mode (readline-style). It
        // must NOT fall through to normal-mode's bare-`u` half-page-up
        // navigation.
        assert!(!search_delegates_ctrl_chord(KeyCode::Char('u'), true));
    }

    // --- search_edit_key_exits_when_empty ---

    #[test]
    fn ctrl_u_exits_search_when_query_empty() {
        // On an empty query, Ctrl+U has nothing to clear, so it doubles as
        // an exit — the same "press again to leave" behavior as Backspace.
        assert!(search_edit_key_exits_when_empty(KeyCode::Char('u'), true));
    }

    #[test]
    fn backspace_exits_search_when_query_empty() {
        // The pre-existing behavior Ctrl+U now mirrors.
        assert!(search_edit_key_exits_when_empty(KeyCode::Backspace, false));
    }

    #[test]
    fn bare_u_does_not_exit_empty_search() {
        // Without ctrl, `u` is query text — it never exits search.
        assert!(!search_edit_key_exits_when_empty(KeyCode::Char('u'), false));
    }

    #[test]
    fn bare_letter_does_not_exit_empty_search() {
        assert!(!search_edit_key_exits_when_empty(KeyCode::Char('t'), false));
    }

    #[test]
    fn ctrl_letter_chords_delegate() {
        // Ctrl-modified chords (e.g. Ctrl+D → mark complete, Ctrl+Enter →
        // task actions modal) fall through to normal-mode handling when
        // typed inside `/`.
        assert!(search_delegates_ctrl_chord(KeyCode::Char('r'), true));
        assert!(search_delegates_ctrl_chord(KeyCode::Enter, true));
    }

    #[test]
    fn bare_letter_stays_in_search_mode() {
        // Without ctrl, letters are text input for the query — never
        // a normal-mode shortcut.
        assert!(!search_delegates_ctrl_chord(KeyCode::Char('t'), false));
        assert!(!search_delegates_ctrl_chord(KeyCode::Char('a'), false));
    }

    // --- view_shortcut ---

    #[test]
    fn view_shortcut_bare_letters_map_to_views() {
        assert_eq!(view_shortcut(KeyCode::Char('t'), false), Some(View::Today));
        assert_eq!(view_shortcut(KeyCode::Char('m'), false), Some(View::Mit));
        assert_eq!(view_shortcut(KeyCode::Char('p'), false), Some(View::PastDue));
        assert_eq!(view_shortcut(KeyCode::Char('w'), false), Some(View::Week));
        assert_eq!(view_shortcut(KeyCode::Char('a'), false), Some(View::All));
    }

    #[test]
    fn view_shortcut_ctrl_modified_never_switches_views() {
        // Ctrl+<letter> must not switch views — otherwise Ctrl+P would
        // collide with the command-palette chord.
        for c in ['t', 'm', 'p', 'w', 'a'] {
            assert_eq!(view_shortcut(KeyCode::Char(c), true), None);
        }
    }

    #[test]
    fn view_shortcut_ignores_unrelated_keys() {
        assert_eq!(view_shortcut(KeyCode::Char('z'), false), None);
        assert_eq!(view_shortcut(KeyCode::Enter, false), None);
    }

    // --- ctrl_opens_palette ---

    #[test]
    fn ctrl_p_opens_the_palette() {
        assert!(ctrl_opens_palette(KeyCode::Char('p')));
        assert!(ctrl_opens_palette(KeyCode::Char('P')));
    }

    #[test]
    fn ctrl_k_no_longer_opens_the_palette() {
        assert!(!ctrl_opens_palette(KeyCode::Char('k')));
        assert!(!ctrl_opens_palette(KeyCode::Char('t')));
    }

    // --- ctrl_quits ---

    #[test]
    fn ctrl_q_quits_the_shell() {
        assert!(ctrl_quits(KeyCode::Char('q'), true));
        assert!(ctrl_quits(KeyCode::Char('Q'), true));
    }

    #[test]
    fn bare_q_is_not_the_global_quit_chord() {
        // Bare `q` quits too, but via the normal-mode handler (tasks panel
        // only). The global chord requires Ctrl so it also reaches us from
        // the brain panel, where bare `q` is forwarded to claude.
        assert!(!ctrl_quits(KeyCode::Char('q'), false));
        assert!(!ctrl_quits(KeyCode::Char('Q'), false));
    }

    #[test]
    fn ctrl_other_keys_do_not_quit() {
        assert!(!ctrl_quits(KeyCode::Char('c'), true));
        assert!(!ctrl_quits(KeyCode::Char('x'), true));
    }

    // --- ctrl_opens_links ---

    #[test]
    fn ctrl_o_opens_links() {
        assert!(ctrl_opens_links(KeyCode::Char('o'), true));
        assert!(ctrl_opens_links(KeyCode::Char('O'), true));
    }

    #[test]
    fn bare_o_does_not_open_links() {
        // Without ctrl, `o` is an ordinary key — never the open action.
        assert!(!ctrl_opens_links(KeyCode::Char('o'), false));
        assert!(!ctrl_opens_links(KeyCode::Char('O'), false));
    }

    #[test]
    fn ctrl_l_no_longer_opens_links() {
        // The binding moved off Ctrl+L; bare `l` (notes toggle) and Ctrl+L
        // must not trigger the open action.
        assert!(!ctrl_opens_links(KeyCode::Char('l'), true));
        assert!(!ctrl_opens_links(KeyCode::Char('k'), true));
        assert!(!ctrl_opens_links(KeyCode::Char('d'), true));
    }

    // --- ctrl_removes_task ---

    #[test]
    fn ctrl_backspace_removes_task() {
        assert!(ctrl_removes_task(KeyCode::Backspace, true));
    }

    #[test]
    fn bare_backspace_does_not_remove_task() {
        // Plain Backspace is a no-op in the task list: it's too easy to hit
        // by accident, so removal requires the Ctrl modifier.
        assert!(!ctrl_removes_task(KeyCode::Backspace, false));
    }

    #[test]
    fn ctrl_other_keys_do_not_remove_task() {
        assert!(!ctrl_removes_task(KeyCode::Char('d'), true));
        assert!(!ctrl_removes_task(KeyCode::Delete, true));
    }

    // --- shell_quote ---

    #[test]
    fn shell_quote_wraps_plain_string_in_single_quotes() {
        assert_eq!(shell_quote("hello"), "'hello'");
    }

    #[test]
    fn shell_quote_empty_string_is_two_quotes() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quote() {
        // POSIX trick: close, escape, reopen.
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_quote_escapes_multiple_single_quotes() {
        assert_eq!(shell_quote("'a'b'"), "''\\''a'\\''b'\\'''");
    }

    // --- BrainInputState::finalize ---

    #[test]
    fn finalize_empty_buffer_returns_none() {
        let s = BrainInputState::about("T1".to_owned(), "x".to_owned());
        assert!(s.finalize().is_none());
    }

    #[test]
    fn finalize_whitespace_only_buffer_returns_none() {
        let mut s = BrainInputState::about("T1".to_owned(), "x".to_owned());
        s.buffer = "   \t  ".to_owned();
        assert!(s.finalize().is_none());
    }

    #[test]
    fn finalize_trims_the_buffer_inside_the_context_prefix() {
        let mut s = BrainInputState::about("T1".to_owned(), "x".to_owned());
        s.buffer = "  hi there  ".to_owned();
        assert_eq!(s.finalize().unwrap(), "This message is about T1 (x): hi there");
    }

    #[test]
    fn finalize_with_task_context_includes_id_and_label() {
        let mut s = BrainInputState::about("T123".to_owned(), "Fix login".to_owned());
        s.buffer = "what's the latest?".to_owned();
        assert_eq!(
            s.finalize().unwrap(),
            "This message is about T123 (Fix login): what's the latest?"
        );
    }

    // --- ConfirmState constructors ---

    #[test]
    fn generate_agenda_ctor_uses_correct_kind_and_title() {
        let s = ConfirmState::generate_agenda();
        assert_eq!(s.kind, ConfirmKind::GenerateAgenda);
        assert!(s.title.contains("agenda"));
        assert_eq!(s.focus, ConfirmChoice::Yes, "Yes should be default-focused");
    }

    #[test]
    fn run_triage_ctor_carries_task_id_and_label() {
        let s = ConfirmState::run_triage("H31".to_owned(), "Morning Triage".to_owned());
        assert_eq!(s.kind, ConfirmKind::RunTriage);
        assert_eq!(s.task_id, "H31");
        assert_eq!(s.task_label, "Morning Triage");
    }

    // --- ConfirmChoice: the triage modal alone offers a third "Skip" button ---

    #[test]
    fn triage_confirm_offers_yes_no_skip() {
        let s = ConfirmState::run_triage("H31".to_owned(), "Morning Triage".to_owned());
        assert_eq!(
            s.choices(),
            &[ConfirmChoice::Yes, ConfirmChoice::No, ConfirmChoice::Skip]
        );
        assert!(s.has_skip());
    }

    #[test]
    fn non_triage_confirms_are_yes_no_only() {
        for s in [
            ConfirmState::mark_complete("T1".to_owned(), "x".to_owned()),
            ConfirmState::remove("T1".to_owned(), "x".to_owned()),
            ConfirmState::generate_agenda(),
        ] {
            assert_eq!(s.choices(), &[ConfirmChoice::Yes, ConfirmChoice::No]);
            assert!(!s.has_skip());
        }
    }

    #[test]
    fn confirm_focus_defaults_to_yes() {
        assert_eq!(
            ConfirmState::run_triage("H1".to_owned(), "T".to_owned()).focus,
            ConfirmChoice::Yes
        );
        assert_eq!(
            ConfirmState::mark_complete("T1".to_owned(), "x".to_owned()).focus,
            ConfirmChoice::Yes
        );
    }

    #[test]
    fn triage_focus_walks_all_three_and_clamps_at_the_ends() {
        let mut s = ConfirmState::run_triage("H1".to_owned(), "T".to_owned());
        s.focus_next();
        assert_eq!(s.focus, ConfirmChoice::No);
        s.focus_next();
        assert_eq!(s.focus, ConfirmChoice::Skip);
        s.focus_next();
        assert_eq!(s.focus, ConfirmChoice::Skip, "clamps at the right end");
        s.focus_prev();
        assert_eq!(s.focus, ConfirmChoice::No);
        s.focus_prev();
        assert_eq!(s.focus, ConfirmChoice::Yes);
        s.focus_prev();
        assert_eq!(s.focus, ConfirmChoice::Yes, "clamps at the left end");
    }

    #[test]
    fn binary_confirm_focus_never_reaches_skip() {
        let mut s = ConfirmState::mark_complete("T1".to_owned(), "x".to_owned());
        s.focus_next();
        assert_eq!(s.focus, ConfirmChoice::No);
        s.focus_next();
        assert_eq!(s.focus, ConfirmChoice::No, "no Skip button to move onto");
    }

    #[test]
    fn skip_triage_prompt_uses_the_documented_skip_language() {
        // The brain agent recognizes "skip daily triage" (the /triage +
        // /todo skills' skip trigger) and marks the Morning Triage habit
        // done rather than running a pass. Keep the phrase intact.
        let p = SKIP_TRIAGE_PROMPT.to_lowercase();
        assert!(p.contains("skip daily triage"), "prompt was: {SKIP_TRIAGE_PROMPT}");
    }

    // --- ConfirmIntent: green for constructive, red for destructive ---

    #[test]
    fn mark_complete_is_a_success_intent() {
        // Completing a task is constructive, so the modal reads green.
        let s = ConfirmState::mark_complete("T1".to_owned(), "x".to_owned());
        assert_eq!(s.intent, ConfirmIntent::Success);
    }

    #[test]
    fn remove_is_a_danger_intent() {
        let s = ConfirmState::remove("T1".to_owned(), "x".to_owned());
        assert_eq!(s.intent, ConfirmIntent::Danger);
    }

    #[test]
    fn agenda_and_triage_are_success_intents() {
        assert_eq!(ConfirmState::generate_agenda().intent, ConfirmIntent::Success);
        assert_eq!(
            ConfirmState::run_triage("H1".to_owned(), "Triage".to_owned()).intent,
            ConfirmIntent::Success
        );
    }

    #[test]
    fn intent_accents_differ_success_green_danger_red() {
        // The two intents must map to distinct accents (green vs red).
        assert_ne!(ConfirmIntent::Success.accent(), ConfirmIntent::Danger.accent());
        // Sanity: green channel dominates for Success, red for Danger.
        assert_eq!(ConfirmIntent::Success.accent(), Color::Rgb(158, 206, 106));
        assert_eq!(ConfirmIntent::Danger.accent(), Color::Rgb(247, 118, 142));
    }

    // --- mouse-scroll panel hit-testing ---

    #[test]
    fn panel_at_returns_tasks_when_no_brain_panel() {
        // Full-width tasks: every coordinate routes to the tasks panel.
        assert_eq!(panel_at(None, 0, 0), Panel::Tasks);
        assert_eq!(panel_at(None, 79, 23), Panel::Tasks);
    }

    #[test]
    fn panel_at_splits_on_the_brain_rect() {
        use ratatui::layout::Rect;
        // 80-col split: tasks on the left, brain occupying x=40..80.
        let brain = Some(Rect { x: 40, y: 0, width: 40, height: 24 });
        // A column inside the brain rect → Brain.
        assert_eq!(panel_at(brain, 50, 10), Panel::Brain);
        assert_eq!(panel_at(brain, 40, 0), Panel::Brain);
        // A column left of the brain rect → Tasks.
        assert_eq!(panel_at(brain, 39, 10), Panel::Tasks);
        assert_eq!(panel_at(brain, 0, 0), Panel::Tasks);
    }

    // --- Alt+U / Alt+D half-page scroll step ---

    #[test]
    fn half_page_step_is_half_the_visible_rows() {
        // A 40-row pane scrolls 20 rows per Alt+U/Alt+D.
        assert_eq!(half_page_step(40), 20);
        assert_eq!(half_page_step(41), 20);
    }

    #[test]
    fn half_page_step_never_falls_below_one_on_tiny_panes() {
        // A 0- or 1-row pane must still advance by a full row, never freeze.
        assert_eq!(half_page_step(0), 1);
        assert_eq!(half_page_step(1), 1);
    }

    // --- deferred brain-submit countdown ---

    #[test]
    fn submit_countdown_is_quiet_when_nothing_is_pending() {
        assert_eq!(advance_submit_countdown(0), (0, false));
    }

    #[test]
    fn submit_countdown_fires_the_return_exactly_once() {
        // A two-tick delay: the first tick just decrements…
        let (after_first, fire_first) = advance_submit_countdown(2);
        assert_eq!((after_first, fire_first), (1, false));
        // …the second tick lands at zero and fires the submitting Return…
        let (after_second, fire_second) = advance_submit_countdown(after_first);
        assert_eq!((after_second, fire_second), (0, true));
        // …and once at zero it stays quiet, so the Enter is sent only once.
        assert_eq!(advance_submit_countdown(after_second), (0, false));
    }
