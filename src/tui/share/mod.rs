//! Captive sharing flow. Slow work runs in a workspace-pinned child process.
mod draw;
mod worker;
pub(crate) use draw::draw;

use crate::share::ShareResult;
use crate::tui::App;
use crate::tui::overlay::{Overlay, close_overlay, open_overlay};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::{Path, PathBuf};
use std::time::Duration;
use worker::Worker;

pub(crate) enum SharePhase {
    Path,
    Expiration(PathBuf),
    Running(Worker),
    Ready(ShareResult),
}

pub(crate) struct ShareState {
    pub(crate) phase: SharePhase,
    pub(crate) buffer: String,
    pub(crate) error: Option<String>,
    pub(crate) progress: String,
    pub(crate) copied: bool,
    cwd: PathBuf,
}

impl ShareState {
    fn new(path: Option<PathBuf>, cwd: PathBuf) -> Self {
        Self {
            phase: path.map_or(SharePhase::Path, SharePhase::Expiration),
            buffer: String::new(),
            error: None,
            progress: String::new(),
            copied: false,
            cwd,
        }
    }
    pub(crate) fn duration(&self) -> Result<Duration> {
        crate::share::expiration(&self.buffer)
    }
}

impl App {
    pub(crate) fn open_share_input(&mut self, path: Option<&Path>) {
        match std::env::current_dir() {
            Ok(cwd) => {
                open_overlay(
                    &mut self.overlay,
                    Overlay::Share(ShareState::new(path.map(Path::to_path_buf), cwd)),
                );
            }
            Err(error) => self
                .status
                .set_flash(crate::tui::modal_state::FlashKind::Error(format!(
                    "Cannot resolve current directory: {error}"
                ))),
        }
    }

    pub(crate) fn submit_share(&mut self) {
        let Some(Overlay::Share(state)) = &mut self.overlay else {
            return;
        };
        let choosing_path = matches!(state.phase, SharePhase::Path);
        let result = match &state.phase {
            SharePhase::Path => crate::share::resolve_path(
                Path::new(state.buffer.trim()),
                &state.cwd,
                self.context.workspace_root(),
            )
            .map(|path| Some(SharePhase::Expiration(path))),
            SharePhase::Expiration(path) => state.duration().and_then(|duration| {
                let path =
                    crate::share::resolve_path(path, &state.cwd, self.context.workspace_root())?;
                Worker::start(self.context.workspace(), &path, duration)
                    .map(|worker| Some(SharePhase::Running(worker)))
            }),
            SharePhase::Ready(result) => copy_link(&result.url).map(|()| {
                state.copied = true;
                None
            }),
            SharePhase::Running(_) => return,
        };
        match result {
            Ok(next) => {
                state.error = None;
                if let Some(phase) = next {
                    state.phase = phase;
                    "Preparing to share...".clone_into(&mut state.progress);
                    if choosing_path {
                        state.buffer.clear();
                    }
                }
            }
            Err(error) => state.error = Some(format!("{error:#}")),
        }
    }

    pub(crate) fn tick_share(&mut self) {
        let Some(Overlay::Share(state)) = &mut self.overlay else {
            return;
        };
        let SharePhase::Running(worker) = &mut state.phase else {
            return;
        };
        match worker.poll(&mut state.progress) {
            Ok(Some(result)) => {
                state.error = copy_link(&result.url).err().map(|_| {
                    "Clipboard unavailable. The link is shown below; Enter retries copying."
                        .to_owned()
                });
                state.copied = state.error.is_none();
                state.phase = SharePhase::Ready(result);
                self.search_refresh();
            }
            Ok(None) => {}
            Err(error) => {
                // Preserve the target and duration so the user can retry without retyping.
                let path = worker.path.clone();
                state.phase = SharePhase::Expiration(path);
                state.error = Some(format!("{error:#}"));
            }
        }
    }
}

fn copy_link(url: &str) -> Result<()> {
    crate::open_target::copy_to_clipboard(Path::new(url)).context("Could not copy download link")
}

pub(crate) fn handle_key(app: &mut App, key: &KeyEvent) {
    let Some(Overlay::Share(state)) = &mut app.overlay else {
        return;
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Char('c' | 'C') if ctrl => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Enter => app.submit_share(),
        _ if !matches!(state.phase, SharePhase::Path | SharePhase::Expiration(_)) => {}
        KeyCode::Char('u' | 'U') if ctrl => {
            state.buffer.clear();
            state.error = None;
        }
        KeyCode::Backspace => {
            state.buffer.pop();
            state.error = None;
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
                && !character.is_control() =>
        {
            state.buffer.push(character);
            state.error = None;
        }
        _ => {}
    }
}
