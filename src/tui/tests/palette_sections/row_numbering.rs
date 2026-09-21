#[test]
fn palette_rows_are_numbered_from_one_in_canonical_order() {
    // Numbers are the 1-based position in the full list, stable regardless of
    // the text filter — so the digit a user types always points at the same
    // command.
    let state = palette(&shared_workspace());
    let rows = state.rows();
    assert_eq!(rows[0].number, 1);
    assert_eq!(rows[1].number, 2);
    assert_eq!(rows.last().unwrap().number, rows.len());
}

#[test]
fn typing_a_row_number_filters_to_that_numbered_row() {
    // "2." prefixes the second command, so a query of "2" keeps it.
    let mut state = palette(&shared_workspace());
    let second = state.rows()[1].action;
    state.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('2'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(
        state.visible().iter().any(|row| row.action == second),
        "typing the row number should surface that numbered command"
    );
}

#[test]
fn the_palette_is_long_enough_to_need_its_viewport() {
    // A guard on the merge itself: one palette now carries every command from
    // every view, so the renderer must scroll rather than clip.
    assert!(
        palette(&shared_workspace()).rows().len() > 40,
        "the merged catalog should be the parent set of every command"
    );
}
