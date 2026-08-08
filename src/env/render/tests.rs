use std::path::PathBuf;

use super::format_breakdown;
use crate::env::breakdown::{Breakdown, VarDoc, WorkspaceEnv};
use crate::settings::Resolved;
use crate::theme::Theme;

fn resolved(name: &str, value: Option<&str>, description: &str) -> Resolved {
    Resolved {
        name: name.to_owned(),
        value: value.map(str::to_owned),
        description: description.to_owned(),
    }
}

fn breakdown() -> Breakdown {
    Breakdown {
        registry_path: PathBuf::from("/home/tester/.config/brain/env.json"),
        global: vec![
            resolved("default_workspace", Some("brain"), "Selected by default."),
            resolved("schema_version", Some("2"), "Registry schema version."),
        ],
        workspaces: vec![
            WorkspaceEnv {
                name: "brain".to_owned(),
                is_default: true,
                is_selected: true,
                rows: vec![
                    resolved("root", Some("/home/tester/brain"), "Workspace root."),
                    resolved("resend_api_key", None, "Resend key."),
                    resolved("sync.b2_bucket", Some("brain-bucket"), "Nested."),
                ],
            },
            WorkspaceEnv {
                name: "family".to_owned(),
                is_default: false,
                is_selected: false,
                rows: vec![resolved(
                    "root",
                    Some("/home/tester/family"),
                    "Workspace root.",
                )],
            },
        ],
        variables: vec![
            VarDoc {
                name: "schema_version".to_owned(),
                description: "Registry schema version.".to_owned(),
            },
            VarDoc {
                name: "root".to_owned(),
                description: "Workspace root on THIS machine.".to_owned(),
            },
        ],
    }
}

fn plain() -> String {
    format_breakdown(&breakdown(), Theme::dark(false))
}

#[test]
fn the_view_has_a_global_section_a_workspaces_section_and_a_legend() {
    let out = plain();

    let global = out.find("Global").expect("Global heading");
    let workspaces = out.find("Workspaces").expect("Workspaces heading");
    let variables = out.find("Variables").expect("Variables heading");
    assert!(global < workspaces, "{out}");
    assert!(workspaces < variables, "{out}");
    assert!(out.contains("/home/tester/.config/brain/env.json"), "{out}");
}

#[test]
fn global_rows_render_above_every_workspace_block() {
    let out = plain();

    let schema_version = out.find("schema_version").expect("global row");
    let first_workspace = out.find("brain (").expect("workspace header");
    assert!(schema_version < first_workspace, "{out}");
}

#[test]
fn each_workspace_is_headed_and_marks_default_and_selected() {
    let out = plain();

    assert!(out.contains("* brain (default, selected)"), "{out}");
    assert!(out.contains("  family"), "{out}");
    assert!(!out.contains("family (default"), "{out}");
}

#[test]
fn a_selected_non_default_workspace_says_only_selected() {
    let mut breakdown = breakdown();
    breakdown.workspaces[0].is_selected = false;
    breakdown.workspaces[1].is_selected = true;

    let out = format_breakdown(&breakdown, Theme::dark(false));

    assert!(out.contains("* brain (default)"), "{out}");
    assert!(out.contains("  family (selected)"), "{out}");
}

#[test]
fn every_row_of_every_workspace_renders_including_unset_ones() {
    let out = plain();

    assert!(out.contains("/home/tester/brain"), "{out}");
    assert!(out.contains("/home/tester/family"), "{out}");
    assert!(out.contains("resend_api_key"), "{out}");
    assert!(out.contains("(unset)"), "{out}");
    assert!(out.contains("sync.b2_bucket"), "{out}");
    // `root` is a row in each of the two blocks, plus one legend entry.
    let root_rows = out
        .lines()
        .filter(|line| line.trim_start().split("  ").next() == Some("root"))
        .count();
    assert_eq!(root_rows, 3, "{out}");
}

#[test]
fn a_blank_line_separates_consecutive_workspace_blocks() {
    let out = plain();

    let family_header = out
        .lines()
        .position(|line| line.trim_start().starts_with("family"))
        .expect("family header");
    let lines = out.lines().collect::<Vec<_>>();
    assert_eq!(lines[family_header - 1], "", "{out}");
}

#[test]
fn an_explicitly_empty_value_reads_as_empty_not_as_blank_padding() {
    let mut breakdown = breakdown();
    breakdown.workspaces[1]
        .rows
        .push(resolved("sync.b2_path", Some(""), "Nested."));

    let out = format_breakdown(&breakdown, Theme::dark(false));

    assert!(out.contains("(empty)"), "{out}");
    assert!(
        out.lines().all(|line| line == line.trim_end()),
        "no line may carry trailing whitespace:\n{out:?}"
    );
}

#[test]
fn the_legend_explains_names_once_and_footnotes_nested_paths() {
    let out = plain();

    let legend = &out[out.find("Variables").expect("Variables heading")..];
    assert!(legend.contains("Workspace root on THIS machine."), "{out}");
    assert!(legend.contains(super::NESTED_NOTE), "{out}");
}

#[test]
fn one_name_column_width_is_shared_by_every_section() {
    let out = plain();

    let value_columns = out
        .lines()
        .filter(|line| line.starts_with(super::ROW_INDENT))
        .filter_map(|line| line.find("  ").map(|_| line.trim_end().to_owned()))
        .collect::<Vec<_>>();
    assert!(!value_columns.is_empty(), "{out}");
    // Every indented row pads its name to the widest name anywhere in the view.
    let widest = "default_workspace".len();
    for line in &value_columns {
        let name = line.trim_start();
        if let Some(rest) = name.split_once("  ") {
            assert!(
                rest.0.len() <= widest,
                "name {:?} wider than the shared column",
                rest.0
            );
        }
    }
    assert!(
        out.contains(&format!(
            "{}{:<widest$}  2",
            super::ROW_INDENT,
            "schema_version"
        )),
        "{out}"
    );
}

#[test]
fn an_empty_registry_view_still_renders_both_sections() {
    let empty = Breakdown {
        registry_path: PathBuf::from("/missing/env.json"),
        global: Vec::new(),
        workspaces: Vec::new(),
        variables: Vec::new(),
    };

    let out = format_breakdown(&empty, Theme::dark(false));

    assert!(out.contains("Global"), "{out}");
    assert!(out.contains("Workspaces"), "{out}");
    assert_eq!(out.matches("(none)").count(), 2, "{out}");
}

#[test]
fn colored_output_paints_headings_names_and_values() {
    let out = format_breakdown(&breakdown(), Theme::dark(true));

    assert!(out.contains("\x1b[1;95m"), "heading painted: {out}");
    assert!(out.contains("\x1b[96m"), "name painted accent: {out}");
    assert!(!plain().contains('\x1b'), "plain theme must stay plain");
}
