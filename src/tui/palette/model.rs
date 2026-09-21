use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Whether a row matches the query: every whitespace-separated word must
/// appear somewhere in the row, in any order, so "brain message" and
/// "message brain" both find "Message brain".
fn matches_query(query: &str, text: &str) -> bool {
    query.split_whitespace().all(|word| text.contains(word))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaletteRow<A> {
    pub(crate) number: usize,
    pub(crate) label: String,
    pub(crate) action: A,
    pub(crate) shortcut: Option<&'static str>,
}

impl<A> PaletteRow<A> {
    pub(crate) fn new(label: impl Into<String>, action: A, shortcut: Option<&'static str>) -> Self {
        Self {
            number: 0,
            label: label.into(),
            action,
            shortcut,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaletteControls {
    wrap_navigation: bool,
    ctrl_pn_navigation: bool,
    ctrl_query_edits: bool,
    uppercase_ctrl_jk: bool,
    allow_alt_text: bool,
}

impl PaletteControls {
    /// The one control set every palette surface uses.
    ///
    /// It is the union of what the two pre-merge palettes each supported, so
    /// no muscle memory was dropped when they became a single surface: word
    /// atoms filter (each typed word must appear somewhere in the row, in any
    /// order), wrapping navigation, both `Ctrl+J/K` and `Ctrl+P/N`, the
    /// readline query edits, and Alt-modified characters as plain text.
    pub(crate) const COMMANDS: Self = Self {
        wrap_navigation: true,
        ctrl_pn_navigation: true,
        ctrl_query_edits: true,
        uppercase_ctrl_jk: true,
        allow_alt_text: true,
    };
}

pub(crate) struct CommandPalette<A> {
    title: String,
    subtitle: Option<String>,
    query: String,
    rows: Vec<PaletteRow<A>>,
    filtered: Vec<usize>,
    selected: usize,
    controls: PaletteControls,
}

impl<A: Copy> CommandPalette<A> {
    pub(crate) fn new(
        title: impl Into<String>,
        subtitle: Option<String>,
        mut rows: Vec<PaletteRow<A>>,
        controls: PaletteControls,
    ) -> Self {
        for (index, row) in rows.iter_mut().enumerate() {
            row.number = index + 1;
        }
        let filtered = (0..rows.len()).collect();
        Self {
            title: title.into(),
            subtitle,
            query: String::new(),
            rows,
            filtered,
            selected: 0,
            controls,
        }
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn subtitle(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[PaletteRow<A>] {
        &self.rows
    }

    pub(crate) const fn selected(&self) -> usize {
        self.selected
    }

    #[cfg(test)]
    pub(crate) fn visible(&self) -> Vec<&PaletteRow<A>> {
        self.filtered
            .iter()
            .map(|&index| &self.rows[index])
            .collect()
    }

    pub(crate) fn selected_action(&self) -> Option<A> {
        self.filtered
            .get(self.selected)
            .map(|&index| self.rows[index].action)
    }

    pub(crate) fn numbered_entries(&self) -> Vec<(String, Option<&'static str>)> {
        self.filtered
            .iter()
            .map(|&index| &self.rows[index])
            .map(|row| (format!("{}. {}", row.number, row.label), row.shortcut))
            .collect()
    }

    fn refilter(&mut self) {
        let query = self.query.to_lowercase();
        self.filtered = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let text = format!("{}. {}", row.number, row.label).to_lowercase();
                matches_query(&query, &text).then_some(index)
            })
            .collect();
        self.selected = 0;
    }

    fn move_up(&mut self) {
        let len = self.filtered.len();
        if len == 0 {
            self.selected = 0;
        } else if self.controls.wrap_navigation {
            self.selected = (self.selected + len - 1) % len;
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    fn move_down(&mut self) {
        let len = self.filtered.len();
        if len == 0 {
            self.selected = 0;
        } else if self.controls.wrap_navigation {
            self.selected = (self.selected + 1) % len;
        } else {
            self.selected = (self.selected + 1).min(len - 1);
        }
    }

    fn append(&mut self, value: char) {
        self.query.push(value);
        self.refilter();
    }

    fn pop(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> PaletteStep<A> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => return PaletteStep::Cancel,
            KeyCode::Char('c') if ctrl => return PaletteStep::Cancel,
            KeyCode::Enter => {
                if let Some(action) = self.selected_action() {
                    return PaletteStep::Confirm(action);
                }
            }
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Char('k') if ctrl => self.move_up(),
            KeyCode::Char('K') if ctrl && self.controls.uppercase_ctrl_jk => self.move_up(),
            KeyCode::Char('j') if ctrl => self.move_down(),
            KeyCode::Char('J') if ctrl && self.controls.uppercase_ctrl_jk => self.move_down(),
            KeyCode::Char('p') if ctrl && self.controls.ctrl_pn_navigation => self.move_up(),
            KeyCode::Char('n') if ctrl && self.controls.ctrl_pn_navigation => self.move_down(),
            KeyCode::Backspace => self.pop(),
            KeyCode::Char('u') if ctrl && self.controls.ctrl_query_edits => {
                self.query.clear();
                self.refilter();
            }
            KeyCode::Char('w') if ctrl && self.controls.ctrl_query_edits => {
                let cut = self
                    .query
                    .trim_end()
                    .rfind(char::is_whitespace)
                    .map_or(0, |index| index + 1);
                self.query.truncate(cut);
                self.refilter();
            }
            KeyCode::Char(value) if !ctrl && (!alt || self.controls.allow_alt_text) => {
                self.append(value);
            }
            _ => {}
        }
        PaletteStep::Continue
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteStep<A> {
    Continue,
    Confirm(A),
    Cancel,
}
