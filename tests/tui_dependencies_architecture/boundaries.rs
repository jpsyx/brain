use std::path::Path;

use super::source::production_tui_sources;
use super::tokens::{
    contains_token_sequence, field_type_count, function_parameter_counts, named_braced_body,
    rust_tokens,
};

#[test]
fn lifetime_guard_distinguishes_app_lifetimes_from_character_literals() {
    let lifetime = "struct App<'a> { value: &'a str }";
    assert!(contains_token_sequence(lifetime, &["App", "<", "'"]));

    let character = "struct App { value: char } fn sample() { let value = 'a'; }";
    assert!(!contains_token_sequence(character, &["App", "<", "'"]));
    assert!(rust_tokens(character).iter().all(|token| token.text != "'"));
}

#[test]
fn final_tui_entry_and_state_boundaries_remain_owned() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = root.join("src/tui");
    let sources = production_tui_sources(&tui_root)
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).expect("read production TUI source");
            (path, source)
        })
        .collect::<Vec<_>>();

    let lifetime_apps = sources
        .iter()
        .filter(|(_, source)| contains_token_sequence(source, &["App", "<", "'"]))
        .map(|(path, _)| path.display().to_string())
        .collect::<Vec<_>>();
    assert!(
        lifetime_apps.is_empty(),
        "App must remain lifetime-free:\n{}",
        lifetime_apps.join("\n")
    );

    let app_source = std::fs::read_to_string(tui_root.join("mod.rs")).expect("read App owner");
    let app_body = named_braced_body(&app_source, "struct", "App").expect("App body");
    assert_eq!(
        field_type_count(app_body, &["Option", "<", "Overlay", ">"]),
        1,
        "App must own exactly one overlay slot"
    );
    assert_eq!(
        field_type_count(
            app_body,
            &[
                "crate",
                "::",
                "tui",
                "::",
                "receiver",
                "::",
                "ReceiverRuntime",
            ],
        ),
        1,
        "App must own exactly one ReceiverRuntime"
    );

    let run_tui_parameter_counts = sources
        .iter()
        .flat_map(|(_, source)| function_parameter_counts(source, "run_tui"))
        .collect::<Vec<_>>();
    assert_eq!(
        run_tui_parameter_counts,
        vec![1],
        "run_tui must have one definition accepting one request"
    );
}
