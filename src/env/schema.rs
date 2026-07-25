//! Declared brain-env variables: what `brain env list` prints, what
//! `brain env set` accepts, and their defaults.

pub(super) struct VarSpec {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) default: Option<&'static str>,
}

/// The brain-env schema, in `brain env list` order. `root` and
/// `markdown_to_pdf_path` are machine-local; the `sync` block is edited via
/// `brain sync setup` (C2), not raw `brain env set`.
pub(super) const VARS: [VarSpec; 2] = [
    VarSpec {
        name: "root",
        description: "Absolute or ~-relative path to the brain (PARA) directory on THIS machine. Defaults to ~/brain; a legacy ~/.config/brain-root pointer is migrated into this key.",
        default: Some("~/brain"),
    },
    VarSpec {
        name: "markdown_to_pdf_path",
        description: "Path to the markdown-to-pdf command on THIS machine. Auto-discovered on first run; required for the Create-PDF action.",
        default: None,
    },
];

pub(super) fn is_known(name: &str) -> bool {
    VARS.iter().any(|v| v.name == name)
}

pub(super) fn default_of(name: &str) -> Option<&'static str> {
    VARS.iter().find(|v| v.name == name).and_then(|v| v.default)
}

pub(super) fn known_names() -> String {
    VARS.iter().map(|v| v.name).collect::<Vec<_>>().join(", ")
}
