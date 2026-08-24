//! Splitting an agenda markdown file into a preamble plus `## ` sections, and
//! reassembling it byte-for-byte.
//!
//! The whole point of the split is *preservation*: a mutation only ever
//! rewrites the section bodies it owns, so the title, `**Load:**`,
//! `**Bottom line:**`, and any section this code knows nothing about come out
//! of [`Document::render`] exactly as they went in.

/// One `## ` heading plus every line up to the next `## ` heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Section {
    pub(super) heading: String,
    pub(super) body: Vec<String>,
}

/// A parsed agenda file: everything before the first `## ` heading, then the
/// sections, plus whether the source ended with a newline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Document {
    pub(super) preamble: Vec<String>,
    pub(super) sections: Vec<Section>,
    trailing_newline: bool,
}

impl Document {
    /// Parse `text`. Headings deeper than `## ` (e.g. `### foo`) stay body
    /// content, so a sub-heading never splits its parent section.
    pub(super) fn parse(text: &str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let mut index = 0;
        let mut preamble = Vec::new();
        while let Some(line) = lines.get(index).filter(|line| !is_heading(line)) {
            preamble.push((*line).to_owned());
            index += 1;
        }
        let mut sections = Vec::new();
        while let Some(heading) = lines.get(index) {
            index += 1;
            let mut body = Vec::new();
            while let Some(line) = lines.get(index).filter(|line| !is_heading(line)) {
                body.push((*line).to_owned());
                index += 1;
            }
            sections.push(Section {
                heading: (*heading).to_owned(),
                body,
            });
        }
        Self {
            preamble,
            sections,
            trailing_newline: text.ends_with('\n'),
        }
    }

    /// Reassemble the document, restoring the original trailing newline.
    pub(super) fn render(&self) -> String {
        let mut out: Vec<&str> = self.preamble.iter().map(String::as_str).collect();
        for section in &self.sections {
            out.push(&section.heading);
            out.extend(section.body.iter().map(String::as_str));
        }
        let mut text = out.join("\n");
        if self.trailing_newline {
            text.push('\n');
        }
        text
    }

    /// Index of the first section whose heading starts with `prefix`.
    pub(super) fn find(&self, prefix: &str) -> Option<usize> {
        self.sections
            .iter()
            .position(|section| section.heading.starts_with(prefix))
    }

    /// Replace the section matched by `prefix` with `replacement`.
    ///
    /// With no match, a `Some` replacement is inserted before the generic
    /// caller-content boundary when one exists (so re-derived core sections
    /// never land after an appended appendix) and appended otherwise. A `None`
    /// replacement removes the matched section.
    pub(super) fn replace_or_set(&mut self, prefix: &str, replacement: Option<Section>) {
        match (self.find(prefix), replacement) {
            (Some(index), Some(section)) => self.sections[index] = section,
            (Some(index), None) => {
                self.sections.remove(index);
            }
            (None, Some(section)) => {
                let index = self
                    .sections
                    .iter()
                    .position(|existing| existing.heading == super::APPENDIX_HEADING)
                    .unwrap_or(self.sections.len());
                self.separate_before(index);
                self.sections.insert(index, section);
            }
            (None, None) => {}
        }
    }

    /// Guarantee a blank line before the section about to be inserted at
    /// `index`, so a new heading never abuts the previous section's last line.
    fn separate_before(&mut self, index: usize) {
        let previous = match index.checked_sub(1) {
            Some(previous) => {
                let section = &mut self.sections[previous];
                if section.body.is_empty() {
                    section.body.push(String::new());
                    return;
                }
                &mut section.body
            }
            None if self.preamble.is_empty() => return,
            None => &mut self.preamble,
        };
        if previous.last().is_some_and(|line| !line.trim().is_empty()) {
            previous.push(String::new());
        }
    }
}

fn is_heading(line: &str) -> bool {
    line.starts_with("## ")
}
