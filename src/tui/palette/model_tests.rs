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

fn search_palette() -> CommandPalette<Action> {
    CommandPalette::new("Test palette", None, rows(), PaletteControls::SEARCH)
}

fn palette_with_label(label: &str, controls: PaletteControls) -> CommandPalette<Action> {
    CommandPalette::new(
        "Test palette",
        None,
        vec![PaletteRow::new(label, Action::Alpha, None)],
        controls,
    )
}

#[test]
fn palette_numbers_rows_and_starts_on_the_first_action() {
    let palette = search_palette();

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
    let mut palette = search_palette();
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
    let mut palette = search_palette();
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
fn clamped_and_wrapping_movement_preserve_each_surface_contract() {
    let mut clamped = search_palette();
    clamped.handle_key(key(KeyCode::Up));
    assert_eq!(clamped.selected_action(), Some(Action::Alpha));
    for _ in 0..4 {
        clamped.handle_key(key(KeyCode::Down));
    }
    assert_eq!(clamped.selected_action(), Some(Action::Gamma));

    let mut wrapping = CommandPalette::new("Task palette", None, rows(), PaletteControls::TASKS);
    wrapping.handle_key(key(KeyCode::Up));
    assert_eq!(wrapping.selected_action(), Some(Action::Gamma));
    wrapping.handle_key(key(KeyCode::Down));
    assert_eq!(wrapping.selected_action(), Some(Action::Alpha));
}

#[test]
fn query_edits_keep_selection_clamped_to_the_visible_rows() {
    let mut palette = search_palette();
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
    let mut confirmed = search_palette();
    confirmed.handle_key(key(KeyCode::Down));
    assert_eq!(
        confirmed.handle_key(key(KeyCode::Enter)),
        PaletteStep::Confirm(Action::Beta)
    );

    let mut escaped = search_palette();
    assert_eq!(escaped.handle_key(key(KeyCode::Esc)), PaletteStep::Cancel);
    assert_eq!(
        escaped.handle_key(ctrl_key(KeyCode::Char('c'))),
        PaletteStep::Cancel
    );
}

#[test]
fn each_surface_keeps_its_existing_ctrl_aliases() {
    let mut search = search_palette();
    search.handle_key(ctrl_key(KeyCode::Char('n')));
    assert_eq!(search.selected_action(), Some(Action::Beta));
    search.handle_key(ctrl_key(KeyCode::Char('p')));
    assert_eq!(search.selected_action(), Some(Action::Alpha));

    let mut tasks = CommandPalette::new("Task palette", None, rows(), PaletteControls::TASKS);
    tasks.handle_key(ctrl_key(KeyCode::Char('n')));
    assert_eq!(tasks.selected_action(), Some(Action::Alpha));
    tasks.handle_key(ctrl_key(KeyCode::Char('j')));
    assert_eq!(tasks.selected_action(), Some(Action::Beta));
}

#[test]
fn uppercase_ctrl_navigation_remains_task_only() {
    let mut search = search_palette();
    search.handle_key(ctrl_key(KeyCode::Char('J')));
    assert_eq!(search.selected_action(), Some(Action::Alpha));

    let mut tasks = CommandPalette::new("Task palette", None, rows(), PaletteControls::TASKS);
    tasks.handle_key(ctrl_key(KeyCode::Char('J')));
    assert_eq!(tasks.selected_action(), Some(Action::Beta));
}

#[test]
fn alt_characters_keep_each_surface_filter_contract() {
    let mut search = search_palette();
    search.handle_key(alt_key(KeyCode::Char('b')));
    assert_eq!(search.query(), "");

    let mut tasks = CommandPalette::new("Task palette", None, rows(), PaletteControls::TASKS);
    tasks.handle_key(alt_key(KeyCode::Char('b')));
    assert_eq!(tasks.query(), "b");
    assert_eq!(tasks.selected_action(), Some(Action::Beta));
}

#[test]
fn search_and_task_palettes_keep_distinct_filter_policies() {
    let cases = [
        (PaletteControls::SEARCH, "brain message", 1),
        (PaletteControls::TASKS, "brain message", 0),
        (PaletteControls::TASKS, "message brain", 1),
        (PaletteControls::TASKS, "MESSAGE BRAIN", 1),
    ];

    for (controls, query, expected_rows) in cases {
        let mut palette = palette_with_label("Message brain", controls);
        for value in query.chars() {
            palette.handle_key(key(KeyCode::Char(value)));
        }
        assert_eq!(palette.visible().len(), expected_rows, "{controls:?}");
    }
}

#[test]
fn search_filter_is_case_insensitive_and_empty_query_restores_every_row() {
    let mut palette = search_palette();
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
fn search_plain_jk_enter_text_and_lowercase_ctrl_jk_navigate() {
    for value in ['j', 'k'] {
        let mut palette = search_palette();
        palette.handle_key(key(KeyCode::Char(value)));
        assert_eq!(palette.query(), value.to_string());
    }

    let cases = [('j', false, Action::Beta), ('k', true, Action::Alpha)];
    for (value, start_on_second, expected) in cases {
        let mut palette = search_palette();
        if start_on_second {
            palette.handle_key(key(KeyCode::Down));
        }
        palette.handle_key(ctrl_key(KeyCode::Char(value)));
        assert_eq!(palette.selected_action(), Some(expected), "Ctrl-{value}");
        assert_eq!(palette.query(), "");
    }
}
