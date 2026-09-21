use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{CommandPalette, PaletteControls, PaletteRow, PaletteStep};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Alpha,
    Beta,
    Gamma,
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn alt_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

fn rows() -> Vec<PaletteRow<Action>> {
    vec![
        PaletteRow::new("Alpha command", Action::Alpha, Some("^A")),
        PaletteRow::new("Beta command", Action::Beta, None),
        PaletteRow::new("Gamma command", Action::Gamma, None),
    ]
}

fn palette() -> CommandPalette<Action> {
    CommandPalette::new("Test palette", None, rows(), PaletteControls::COMMANDS)
}

fn palette_with_label(label: &str) -> CommandPalette<Action> {
    CommandPalette::new(
        "Test palette",
        None,
        vec![PaletteRow::new(label, Action::Alpha, None)],
        PaletteControls::COMMANDS,
    )
}

#[test]
fn palette_numbers_rows_and_starts_on_the_first_action() {
    let palette = palette();

    assert_eq!(
        palette
            .rows()
            .iter()
            .map(|row| row.number)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(palette.selected_action(), Some(Action::Alpha));
}

#[test]
fn filtering_matches_number_and_words_and_resets_selection() {
    let mut palette = palette();
    assert_eq!(
        palette.handle_key(key(KeyCode::Down)),
        PaletteStep::Continue
    );
    assert_eq!(palette.selected_action(), Some(Action::Beta));

    for c in "3 gamma".chars() {
        assert_eq!(
            palette.handle_key(key(KeyCode::Char(c))),
            PaletteStep::Continue
        );
    }

    assert_eq!(palette.selected(), 0);
    assert_eq!(palette.selected_action(), Some(Action::Gamma));
    assert_eq!(palette.visible().len(), 1);
}

#[test]
fn empty_results_have_no_selection_and_enter_does_not_confirm() {
    let mut palette = palette();
    for c in "missing".chars() {
        palette.handle_key(key(KeyCode::Char(c)));
    }

    assert!(palette.visible().is_empty());
    assert_eq!(palette.selected_action(), None);
    assert_eq!(
        palette.handle_key(key(KeyCode::Enter)),
        PaletteStep::Continue
    );
}

#[test]
fn navigation_wraps_at_both_ends() {
    // One surface, one contract: the merged palette wraps, so `Up` from the
    // first row reaches the last without a long scroll back through 50-odd
    // commands.
    let mut palette = palette();
    palette.handle_key(key(KeyCode::Up));
    assert_eq!(palette.selected_action(), Some(Action::Gamma));
    palette.handle_key(key(KeyCode::Down));
    assert_eq!(palette.selected_action(), Some(Action::Alpha));
}

#[test]
fn query_edits_keep_selection_clamped_to_the_visible_rows() {
    let mut palette = palette();
    palette.handle_key(key(KeyCode::Down));
    palette.handle_key(key(KeyCode::Down));
    palette.handle_key(key(KeyCode::Char('b')));
    assert_eq!(palette.selected_action(), Some(Action::Beta));

    palette.handle_key(key(KeyCode::Backspace));
    assert_eq!(palette.selected_action(), Some(Action::Alpha));
    palette.handle_key(key(KeyCode::Char('g')));
    palette.handle_key(ctrl_key(KeyCode::Char('u')));
    assert_eq!(palette.query(), "");
    assert_eq!(palette.selected_action(), Some(Action::Alpha));

    for c in "alpha command".chars() {
        palette.handle_key(key(KeyCode::Char(c)));
    }
    palette.handle_key(ctrl_key(KeyCode::Char('w')));
    assert_eq!(palette.query(), "alpha ");
}

#[test]
fn enter_confirms_and_escape_or_ctrl_c_cancel() {
    let mut confirmed = palette();
    confirmed.handle_key(key(KeyCode::Down));
    assert_eq!(
        confirmed.handle_key(key(KeyCode::Enter)),
        PaletteStep::Confirm(Action::Beta)
    );

    let mut escaped = palette();
    assert_eq!(escaped.handle_key(key(KeyCode::Esc)), PaletteStep::Cancel);
    assert_eq!(
        escaped.handle_key(ctrl_key(KeyCode::Char('c'))),
        PaletteStep::Cancel
    );
}

#[test]
fn both_ctrl_navigation_aliases_work_in_either_case() {
    // The merged controls are the union of what the two pre-merge palettes
    // each accepted, so no one's muscle memory was dropped.
    let cases = [
        (ctrl_key(KeyCode::Char('n')), Action::Beta),
        (ctrl_key(KeyCode::Char('j')), Action::Beta),
        (ctrl_key(KeyCode::Char('J')), Action::Beta),
    ];
    for (chord, expected) in cases {
        let mut palette = palette();
        palette.handle_key(chord);
        assert_eq!(palette.selected_action(), Some(expected), "{chord:?}");
    }

    let cases = [
        ctrl_key(KeyCode::Char('p')),
        ctrl_key(KeyCode::Char('k')),
        ctrl_key(KeyCode::Char('K')),
    ];
    for chord in cases {
        let mut palette = palette();
        palette.handle_key(key(KeyCode::Down));
        palette.handle_key(chord);
        assert_eq!(palette.selected_action(), Some(Action::Alpha), "{chord:?}");
    }
}

#[test]
fn alt_modified_characters_are_plain_filter_text() {
    let mut palette = palette();
    palette.handle_key(alt_key(KeyCode::Char('b')));
    assert_eq!(palette.query(), "b");
    assert_eq!(palette.selected_action(), Some(Action::Beta));
}

#[test]
fn typed_words_match_in_any_order_and_ignore_case() {
    let cases = [
        ("brain message", 1),
        ("message brain", 1),
        ("MESSAGE BRAIN", 1),
        ("message missing", 0),
    ];

    for (query, expected_rows) in cases {
        let mut palette = palette_with_label("Message brain");
        for value in query.chars() {
            palette.handle_key(key(KeyCode::Char(value)));
        }
        assert_eq!(palette.visible().len(), expected_rows, "{query:?}");
    }
}

#[test]
fn an_empty_query_restores_every_row() {
    let mut palette = palette();
    for value in "ALPHA".chars() {
        palette.handle_key(key(KeyCode::Char(value)));
    }
    assert_eq!(palette.selected_action(), Some(Action::Alpha));
    assert_eq!(palette.visible().len(), 1);

    for _ in "ALPHA".chars() {
        palette.handle_key(key(KeyCode::Backspace));
    }
    assert_eq!(palette.query(), "");
    assert_eq!(
        palette
            .visible()
            .into_iter()
            .map(|row| row.action)
            .collect::<Vec<_>>(),
        vec![Action::Alpha, Action::Beta, Action::Gamma]
    );
}

#[test]
fn plain_jk_are_filter_text_not_navigation() {
    for value in ['j', 'k'] {
        let mut palette = palette();
        palette.handle_key(key(KeyCode::Char(value)));
        assert_eq!(palette.query(), value.to_string());
    }
}
