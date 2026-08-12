//! The email channel's HTML part: the agent's markdown, actually rendered.
//!
//! The agent writes markdown and an email client renders HTML, so one of the
//! two has to give. Parsing is `pulldown-cmark`'s job; this module owns only
//! the two things a general-purpose renderer cannot decide for us — what is
//! safe to emit when the source quotes a stranger's message, and how a reply
//! should look in a mail client.

use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, html::push_html};

/// URL schemes a reply may hand a reader to click.
///
/// Everything else — `javascript:`, `data:`, `file:`, an unknown scheme — is
/// dropped rather than rewritten. The reply quotes message text written by
/// whoever sent the SMS or email, so a link destination is untrusted input,
/// and a link is the one thing in an email a reader is invited to act on.
const CLICKABLE_SCHEMES: [&str; 3] = ["https://", "http://", "mailto:"];

/// Element styling for the rendered body.
///
/// Inline `style` attributes survive every mail client, but `pulldown-cmark`
/// writes plain semantic tags, and rewriting each one to carry an attribute
/// would mean owning the renderer we just avoided writing. A `<style>` block is
/// the trade: honored by Gmail, Apple Mail, and Outlook.com, and where it is
/// ignored the message is still a correct HTML document — headings, lists, and
/// links intact, merely unstyled. That is the failure we want.
const BODY_STYLE: &str = "\
h1,h2,h3,h4{line-height:1.25;margin:1.5em 0 .5em;font-weight:600}\
h1{font-size:1.5em}h2{font-size:1.25em}h3{font-size:1.1em}\
p,ul,ol,blockquote,table,pre{margin:0 0 1em}\
ul,ol{padding-left:1.4em}li{margin:.25em 0}\
a{color:#3257a8}\
code{background:#f2efe8;border-radius:4px;padding:.1em .35em;font-size:.9em}\
pre{background:#f2efe8;border-radius:8px;padding:12px 14px;overflow-x:auto}\
pre code{background:none;padding:0}\
blockquote{border-left:3px solid #d8d2c4;padding-left:14px;color:#5a5750}\
table{border-collapse:collapse}\
th,td{border:1px solid #ddd8cc;padding:6px 10px;text-align:left}\
hr{border:0;border-top:1px solid #ddd8cc;margin:1.5em 0}\
img{max-width:100%}";

/// Render an agent answer into the email channel's HTML part.
#[must_use]
pub fn email_html(markdown: &str) -> String {
    let body = render(markdown);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>{BODY_STYLE}</style></head>\
<body style=\"margin:0;background:#f6f4ef;padding:32px;font-family:ui-sans-serif,system-ui,sans-serif;color:#252525;line-height:1.55\">\
<main style=\"max-width:680px;margin:auto;background:#fff;padding:32px;border-radius:16px;box-shadow:0 8px 30px #00000012\">{body}</main>\
</body></html>"
    )
}

/// Markdown extensions the agent actually writes.
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_SMART_PUNCTUATION
}

fn render(markdown: &str) -> String {
    let mut body = String::new();
    push_html(
        &mut body,
        Parser::new_ext(markdown.trim(), options()).map(defuse),
    );
    body
}

/// Neutralize the events that would let source text act rather than read.
///
/// Raw HTML becomes the text it looks like, so a reader sees what was written
/// and nothing runs; an unclickable destination is dropped.
fn defuse(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: clickable(link_type, dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: clickable(link_type, dest_url),
            title,
            id,
        }),
        other => other,
    }
}

/// The destination to emit, empty when it is not one a reader may follow.
///
/// An autolinked email address arrives without its `mailto:` scheme, so it is
/// judged by how it was written rather than by its text alone.
fn clickable(link_type: LinkType, dest_url: CowStr<'_>) -> CowStr<'_> {
    if link_type == LinkType::Email {
        return dest_url;
    }
    let lowercased = dest_url.to_ascii_lowercase();
    if CLICKABLE_SCHEMES
        .iter()
        .any(|scheme| lowercased.starts_with(scheme))
    {
        dest_url
    } else {
        CowStr::Borrowed("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent answers in markdown. An email client renders HTML, so markdown
    /// delivered verbatim reads as punctuation noise: hashes before headings,
    /// asterisks around emphasis, hyphens down the left margin, and a link the
    /// reader has to copy out of brackets by hand.
    #[test]
    fn markdown_becomes_real_html_rather_than_its_own_source_text() {
        let html = email_html(
            "## Today\n\n- **Rent** is due\n- see [the invoice](https://example.test/a)\n",
        );
        assert!(html.contains("<h2"), "a heading must be a heading: {html}");
        assert!(html.contains("<ul"), "a list must be a list: {html}");
        assert!(html.contains("<strong>Rent</strong>"), "{html}");
        assert!(
            html.contains("href=\"https://example.test/a\""),
            "a link must be clickable: {html}"
        );
        for marker in ["## ", "**Rent**", "](http"] {
            assert!(
                !html.contains(marker),
                "raw markdown {marker:?} survived into the email: {html}"
            );
        }
    }

    /// Message text is written by whoever sent the SMS or email, and the agent
    /// quotes it back. Rendering markdown must not become a way to get script
    /// or markup of someone else's choosing into the user's mail client.
    #[test]
    fn markup_in_the_answer_is_shown_not_executed() {
        let html = email_html("Reported: <script>alert(1)</script> and <img src=x onerror=y>");
        for tag in ["<script>", "<img src=x"] {
            assert!(!html.contains(tag), "{tag:?} passed through as markup: {html}");
        }
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "{html}");
        assert!(html.contains("&lt;img src=x onerror=y&gt;"), "{html}");
    }

    /// A link is the one place a renderer hands the reader something to click,
    /// so its destination is attacker-influenced input too.
    #[test]
    fn a_link_can_only_point_somewhere_a_reader_can_safely_click() {
        let html = email_html("[click](javascript:alert(1)) [mail](mailto:a@b.test)");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("mailto:a@b.test"), "{html}");
    }

    /// Plain prose is the common case and must survive unchanged, wrapped in
    /// the same styled shell every reply uses.
    #[test]
    fn plain_prose_keeps_its_paragraphs_and_the_styled_shell() {
        let html = email_html("First line.\n\nSecond line.");
        assert!(html.starts_with("<!doctype html>"), "{html}");
        assert_eq!(html.matches("<p>").count(), 2, "{html}");
        assert!(html.contains("First line."));
        assert!(html.contains("max-width"), "the shell must still be styled");
    }

    /// GitHub-flavored markdown is what the agent actually writes.
    #[test]
    fn the_extensions_the_agent_actually_writes_are_enabled() {
        let table = email_html("| a | b |\n| - | - |\n| 1 | 2 |");
        assert!(table.contains("<table"), "{table}");
        assert!(email_html("~~gone~~").contains("<del>"));
        assert!(email_html("- [x] done").contains("checkbox"));
    }
}

