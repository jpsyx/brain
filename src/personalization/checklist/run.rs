//! The `/dev/tty` shell around the pure [`Checklist`](super::Checklist).
//!
//! Sets up raw mode + a ratatui terminal on `/dev/tty` (like the main TUI and
//! the picker), draws the checklist, feeds key presses to the pure state
//! machine, and restores the terminal on the way out. Returns the confirmed
//! selection, `None` on cancel, and `None` when there is no controlling
//! terminal (headless/CI) so callers fall back gracefully. Kept thin: no
//! decision logic lives here.

use std::fs::OpenOptions;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use super::{Checklist, Outcome};

/// Run the checklist interactively on `/dev/tty`. `Ok(Some(selection))` on
/// confirm, `Ok(None)` on cancel or when no terminal is available.
pub fn run_checklist(mut cl: Checklist) -> Result<Option<Vec<String>>> {
    let Ok(tty) = OpenOptions::new().write(true).open("/dev/tty") else {
        return Ok(None); // headless: no interactive selection possible
    };
    enable_raw_mode()?;
    let mut out = tty;
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut cl);

    // Always restore, even if the loop errored.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    result
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    cl: &mut Checklist,
) -> Result<Option<Vec<String>>> {
    loop {
        terminal.draw(|f| draw(f, cl))?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match cl.handle_key(key) {
                Outcome::Continue => {}
                Outcome::Confirm => return Ok(Some(cl.result())),
                Outcome::Cancel => return Ok(None),
            }
        }
    }
}

fn draw(f: &mut ratatui::Frame, cl: &Checklist) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(4)])
        .split(f.area());

    let rows: Vec<ListItem> = cl
        .items
        .iter()
        .map(|it| {
            let mark = if it.checked { "[x]" } else { "[ ]" };
            ListItem::new(Line::from(format!("{mark} {}", it.label)))
        })
        .collect();
    let list = List::new(rows)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", cl.title)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");
    let mut state = ListState::default();
    state.select(Some(cl.cursor));
    f.render_stateful_widget(list, chunks[0], &mut state);

    let footer = cl.create_buffer().map_or_else(
        || {
            vec![
                Line::from("↑/↓ move   space toggle   a add new"),
                Line::from("enter save   esc/q cancel"),
            ]
        },
        |buf| {
            vec![
                Line::from("Create new (comma/semicolon separated):"),
                Line::from(Span::styled(
                    format!("  {buf}▏"),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from("enter add · esc discard"),
            ]
        },
    );
    f.render_widget(
        Paragraph::new(footer).block(Block::default().borders(Borders::ALL)),
        chunks[1],
    );
}
