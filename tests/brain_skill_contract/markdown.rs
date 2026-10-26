use std::collections::BTreeSet;

pub fn prose(markdown: &str) -> String {
    let mut fence = None;
    let mut lines = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|ch| matches!(ch, '`' | '~'));
        let length = marker.map_or(0, |ch| trimmed.chars().take_while(|c| *c == ch).count());
        if let Some((ch, opening_length)) = fence {
            if marker == Some(ch) && length >= opening_length {
                fence = None;
            }
        } else if length >= 3 {
            fence = marker.map(|ch| (ch, length));
        } else {
            lines.push(line);
        }
    }
    lines.join("\n")
}

pub fn links(markdown: &str) -> Vec<String> {
    pulldown_cmark::Parser::new(markdown)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Start(
                pulldown_cmark::Tag::Link { dest_url, .. }
                | pulldown_cmark::Tag::Image { dest_url, .. },
            ) => Some(dest_url.into_string()),
            _ => None,
        })
        .filter(|target| {
            target != "url"
                && !target.contains("://")
                && !target.starts_with(['/', '~'])
                && !target.starts_with("mailto:")
                && !target.contains(['<', '>'])
        })
        .collect()
}

pub fn anchors(markdown: &str) -> BTreeSet<String> {
    use pulldown_cmark::{Event, Tag, TagEnd};
    let mut anchors = BTreeSet::new();
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    let mut heading = None::<String>;
    for event in pulldown_cmark::Parser::new(markdown) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = &mut heading {
                    heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                let title = heading.take().unwrap();
                let slug: String = title
                    .to_lowercase()
                    .chars()
                    .filter(|ch| {
                        ch.is_alphanumeric() || ch.is_whitespace() || matches!(ch, '-' | '_')
                    })
                    .map(|ch| if ch.is_whitespace() { '-' } else { ch })
                    .collect();
                let count = seen.entry(slug.clone()).or_default();
                anchors.insert(if *count == 0 {
                    slug
                } else {
                    format!("{slug}-{count}")
                });
                *count += 1;
            }
            _ => (),
        }
    }
    let explicit = regex::Regex::new(r#"<a\s+(?:id|name)="([^"]+)"\s*></a>"#).unwrap();
    anchors.extend(
        explicit
            .captures_iter(&prose(markdown))
            .map(|capture| capture[1].to_owned()),
    );
    anchors
}

pub fn routes(markdown: &str) -> BTreeSet<String> {
    let pattern = regex::Regex::new(r#"(?:^|[\s`\["'(])/(\p{Ll}[a-z0-9-]*)"#).unwrap();
    let text = prose(markdown);
    pattern
        .captures_iter(&text)
        .filter(|capture| {
            let name = capture.get(1).unwrap();
            if text[name.end()..].chars().next().is_some_and(|ch| {
                !ch.is_whitespace() && !matches!(ch, '`' | '"' | '\'' | ')' | ',' | '.')
            }) {
                return false;
            }
            let start = name.start();
            let prefix = &text[..start - 1];
            !prefix
                .strip_suffix('`')
                .is_some_and(|before| before.chars().last().is_some_and(char::is_alphanumeric))
        })
        .map(|capture| capture[1].to_owned())
        .collect()
}

#[test]
fn parses_real_navigation_but_not_fenced_examples_or_documented_placeholders() {
    let text = "# A `real` heading!\n[real](../other/SKILL.md#a-real-heading)\n[sample](path/<name>.md) [label](url) `[inline sample](invented.md)`\n```md\n[example](missing.md)\n/fake\n# Fake\n```\n~~~\n[example](also-missing.md)\n~~~\nRun `/todo add`, /organize /projects and `/tmp/file`, `id`/timestamp.\n<a id=\"old-anchor\"></a>\n";
    assert_eq!(links(text), ["../other/SKILL.md#a-real-heading"]);
    assert_eq!(
        anchors(text),
        BTreeSet::from(["a-real-heading".to_owned(), "old-anchor".to_owned()])
    );
    assert_eq!(
        routes(text),
        BTreeSet::from([
            "organize".to_owned(),
            "projects".to_owned(),
            "todo".to_owned()
        ])
    );
}
