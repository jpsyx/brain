use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

use super::*;
use crate::tui::Overlay;

const WIDTH: u16 = 120;
const HEIGHT: u16 = 30;
const MAIN_RIGHT_EDGE: u16 = WIDTH / 2 - 1;

fn find_text(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let needle: Vec<char> = needle.chars().collect();
    for y in 0..HEIGHT {
        for x in 0..=WIDTH.saturating_sub(u16::try_from(needle.len()).unwrap()) {
            let matches = needle.iter().enumerate().all(|(offset, expected)| {
                buffer[(x + u16::try_from(offset).unwrap(), y)].symbol() == expected.to_string()
            });
            if matches {
                return Some((x, y));
            }
        }
    }
    None
}

fn rendered_modal_right_edge(overlay: Overlay, title: &str) -> u16 {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.shell
        .show_main_view(crate::main_view::MainView::BrainSearch);
    app.overlay = Some(overlay);
    let (brain, _) = recording_controller(&app, true, "brain panel");
    app.brain.install_main(brain);

    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| crate::tui::draw(frame, &mut app))
        .expect("draw split shell");
    let buffer = terminal.backend().buffer();
    let (title_x, title_y) = find_text(buffer, title).expect("modal title");
    (title_x + u16::try_from(title.chars().count()).unwrap()..WIDTH)
        .find(|x| buffer[(*x, title_y)].symbol() == "╮")
        .expect("modal right border")
}

#[test]
fn search_modals_stay_inside_the_search_half_when_the_brain_panel_is_open() {
    let overlays = [
        (
            Overlay::SearchPalette(crate::menu::SearchPalette::new(
                "Command palette",
                None,
                crate::menu::items(PanelSide::Right, false, &crate::menu::Targets::default()),
                crate::tui::PaletteControls::SEARCH,
            )),
            "Command palette",
        ),
        (
            Overlay::SearchConfirmation(crate::confirm::Confirm::pdf(PathBuf::from("plan.md"))),
            "Create PDF",
        ),
    ];

    for (overlay, title) in overlays {
        let right_edge = rendered_modal_right_edge(overlay, title);
        assert!(
            right_edge <= MAIN_RIGHT_EDGE,
            "{title} crossed from the search panel into the brain panel: right edge {right_edge}"
        );
    }
}
