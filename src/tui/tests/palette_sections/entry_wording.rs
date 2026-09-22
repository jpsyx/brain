#[test]
fn a_highlighted_markdown_file_names_every_entry_row_it_can_satisfy() {
    let state = palette(&with_entry(entry("plan.md", true, true)));

    let cases = [
        (EntryCommand::Open, "Open file 'plan.md'"),
        (EntryCommand::Reveal, "Reveal 'projects/atlas' in Finder"),
        (EntryCommand::Explore, "Open the explorer on 'plan.md'"),
        (EntryCommand::CopyFilePath, "Copy path to 'plan.md'"),
        (EntryCommand::CopyDirPath, "Copy path to 'projects/atlas'"),
        (EntryCommand::CreatePdf, "Create PDF for 'plan.md'"),
        (EntryCommand::Delete, "Delete 'plan.md'"),
    ];
    for (command, expected) in cases {
        assert_eq!(
            label_of(&state, Command::Entry(command)).as_deref(),
            Some(expected),
            "{command:?}"
        );
    }
}

#[test]
fn a_highlighted_directory_leaves_the_file_only_rows_generic() {
    let state = palette(&with_entry(entry("atlas", false, false)));

    assert_eq!(
        label_of(&state, Command::Entry(EntryCommand::Open)).as_deref(),
        Some("Open dir 'projects/atlas'")
    );
    assert_eq!(
        label_of(&state, Command::Entry(EntryCommand::CopyFilePath)).as_deref(),
        Some("Copy a file's path")
    );
    assert_eq!(
        label_of(&state, Command::Entry(EntryCommand::CreatePdf)).as_deref(),
        Some("Create a PDF from a markdown file")
    );
}

#[test]
fn a_highlighted_non_markdown_file_leaves_the_pdf_row_generic() {
    let state = palette(&with_entry(entry("scan.png", true, false)));

    assert_eq!(
        label_of(&state, Command::Entry(EntryCommand::CreatePdf)).as_deref(),
        Some("Create a PDF from a markdown file")
    );
    assert_eq!(
        label_of(&state, Command::Entry(EntryCommand::Delete)).as_deref(),
        Some("Delete 'scan.png'")
    );
}

#[test]
fn nothing_highlighted_makes_every_entry_row_read_generically() {
    let state = palette(&shared_workspace());

    let cases = [
        (EntryCommand::Open, "Open a file or directory"),
        (EntryCommand::Reveal, "Reveal a directory in Finder"),
        (
            EntryCommand::Explore,
            "Open the file explorer on a file or directory",
        ),
        (EntryCommand::CopyFilePath, "Copy a file's path"),
        (EntryCommand::CopyDirPath, "Copy a directory's path"),
        (
            EntryCommand::CreatePdf,
            "Create a PDF from a markdown file",
        ),
        (EntryCommand::Delete, "Delete a file or directory"),
    ];
    for (command, expected) in cases {
        assert_eq!(
            label_of(&state, Command::Entry(command)).as_deref(),
            Some(expected),
            "{command:?}"
        );
    }
}
