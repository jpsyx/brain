use super::{MAX_PROMPT_BYTES, bounded_prompt, html_to_text};

#[test]
fn html_only_mail_becomes_readable_text_instead_of_raw_markup() {
    let html = "<html><head><style>p { color: red }</style>\
                <script>alert('x')</script></head>\
                <body><p>Hello there.</p><p>Second line &amp; more.</p></body></html>";

    let text = html_to_text(html);

    assert!(
        text == "Hello there.\n\nSecond line & more.",
        "HTML mail was not converted to the expected text"
    );
    assert!(!text.contains('<'), "markup must not survive");
    assert!(!text.contains("alert"), "script bodies must not survive");
    assert!(!text.contains("color"), "style bodies must not survive");
}

#[test]
fn line_breaks_and_entities_are_preserved_as_written() {
    assert!(
        html_to_text("<div>One<br>Two<br/>Three</div>") == "One\nTwo\nThree",
        "line breaks were not preserved"
    );
    assert!(
        html_to_text("<p>&lt;tag&gt; &quot;quoted&quot; &#39;apostrophe&#39;&nbsp;end</p>")
            == "<tag> \"quoted\" 'apostrophe' end",
        "HTML entities were not decoded as expected"
    );
}

#[test]
fn a_message_within_the_budget_is_handed_over_untouched() {
    assert!(
        bounded_prompt("A short question.", 100) == "A short question.",
        "an in-budget prompt was changed"
    );
}

#[test]
fn an_oversized_message_is_truncated_and_says_so() {
    let long = "x".repeat(MAX_PROMPT_BYTES * 2);

    let bounded = bounded_prompt(&long, MAX_PROMPT_BYTES);

    assert!(bounded.len() < long.len());
    assert!(
        bounded.contains("truncated"),
        "the agent must be told the message was cut"
    );
    assert!(bounded.starts_with("xxxx"));
}

#[test]
fn truncation_never_splits_a_character() {
    let text = "é".repeat(64);

    let bounded = bounded_prompt(&text, 33);

    assert!(bounded.starts_with("éé"));
    assert!(bounded.contains("truncated"));
}
