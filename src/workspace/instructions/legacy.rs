use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};

const TEMPLATE: &str = include_str!("../../../templates/workspace/legacy/AGENTS-v0.105.3.md");

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovedLegacySpan {
    /// Byte offsets in the original before-image, before any removal.
    pub range: Range<usize>,
    pub bytes: Vec<u8>,
}

pub(super) fn remove_known_sections(text: &str) -> (Vec<u8>, Vec<RemovedLegacySpan>) {
    if text == TEMPLATE {
        return (Vec::new(), vec![removed(text, 0..text.len())]);
    }
    let known = sections(TEMPLATE);
    let observed = sections(text);
    let mut spans = Vec::new();
    for range in &observed {
        let body = &text[range.clone()];
        if known
            .iter()
            .any(|known_range| &TEMPLATE[known_range.clone()] == body)
            && observed
                .iter()
                .filter(|candidate| &text[(*candidate).clone()] == body)
                .count()
                == 1
        {
            spans.push(removed(text, range.clone()));
        }
    }
    let mut retained = Vec::new();
    let mut position = 0;
    for span in &spans {
        retained.extend_from_slice(&text.as_bytes()[position..span.range.start]);
        position = span.range.end;
    }
    retained.extend_from_slice(&text.as_bytes()[position..]);
    (retained, spans)
}

fn removed(text: &str, range: Range<usize>) -> RemovedLegacySpan {
    RemovedLegacySpan {
        bytes: text.as_bytes()[range.clone()].to_vec(),
        range,
    }
}

fn sections(text: &str) -> Vec<Range<usize>> {
    let mut boundaries = vec![0];
    boundaries.extend(
        Parser::new(text)
            .into_offset_iter()
            .filter_map(|(event, range)| {
                if matches!(
                    event,
                    Event::Start(Tag::Heading {
                        level: HeadingLevel::H2,
                        ..
                    })
                ) && range.start > 0
                    && text.as_bytes()[range.start - 1] == b'\n'
                {
                    Some(range.start)
                } else {
                    None
                }
            }),
    );
    boundaries.push(text.len());
    boundaries.windows(2).map(|pair| pair[0]..pair[1]).collect()
}
