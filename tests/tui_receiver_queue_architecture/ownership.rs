#[path = "ownership/aliases.rs"]
mod aliases;
#[path = "ownership/attributes.rs"]
mod attributes;
#[path = "ownership/cfg.rs"]
mod cfg;
#[path = "ownership/identifiers.rs"]
mod identifiers;
#[path = "ownership/macros.rs"]
mod macros;
#[path = "ownership/persistent.rs"]
mod persistent;
#[path = "ownership/receiver_effect.rs"]
mod receiver_effect;
#[path = "ownership/scopes.rs"]
mod scopes;
#[path = "ownership/visitors.rs"]
mod visitors;

use std::collections::HashSet;
use std::path::{Component, Path};

use aliases::visible_job_reexport_renames;
use attributes::AttributeAudit;
use identifiers::{canonical_ident, ident_is};
use persistent::PersistentItem;
use receiver_effect::receiver_effect_payloads_are_one_shot;
use visitors::MentionVisitor;

const EFFECT_PATH: &str = "src/tui/receiver/effect.rs";
const QUEUE_PATH: &str = "src/tui/receiver/queue.rs";

pub(super) fn queue_boundary_violations(source: &str) -> Vec<String> {
    queue_boundary_violations_at(Path::new("src/tui/unrelated.rs"), source)
}

pub(super) fn queue_boundary_violations_at(path: &Path, source: &str) -> Vec<String> {
    if is_test_source(path) || is_exact_manifest_path(path, QUEUE_PATH) {
        return Vec::new();
    }
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(error) => return vec![format!("could not parse production Rust: {error}")],
    };

    let mut guard = OwnershipGuard {
        effect_owner: is_exact_manifest_path(path, EFFECT_PATH),
        violations: Vec::new(),
    };
    let aliases = HashSet::from(["InboundJob".to_owned()]);
    guard.inspect_scope(&file.items, &aliases, true);
    guard.violations
}

struct OwnershipGuard {
    effect_owner: bool,
    violations: Vec<String>,
}

impl OwnershipGuard {
    fn inspect_persistent(
        &mut self,
        item: PersistentItem<'_>,
        aliases: &HashSet<String>,
        top_level: bool,
    ) {
        let mut attributes = AttributeAudit::default();
        item.visit(&mut attributes);
        if let Some(attribute) = attributes.unsupported() {
            self.violations.push(format!(
                "{} {} has unsupported attribute {attribute}",
                item.kind(),
                item.name()
            ));
            return;
        }

        if let PersistentItem::Enum(effect) = item
            && self.effect_owner
            && top_level
            && ident_is(&effect.ident, "ReceiverEffect")
            && receiver_effect_payloads_are_one_shot(effect, aliases)
        {
            return;
        }

        let mut mentions = MentionVisitor::new(aliases);
        item.visit(&mut mentions);
        if mentions.found() {
            self.violations.push(format!(
                "{} {} declares persistent raw InboundJob storage",
                item.kind(),
                item.name()
            ));
        }

        self.inspect_nested_persistent_syntax(item, aliases);
    }

    fn inspect_visible_job_reexport(&mut self, item: &syn::ItemUse, aliases: &HashSet<String>) {
        for (source, rename) in visible_job_reexport_renames(item, aliases) {
            self.violations.push(format!(
                "visible re-export renames raw InboundJob alias {source} as {rename}"
            ));
        }
    }
}

fn is_test_source(path: &Path) -> bool {
    let path = manifest_relative(path).unwrap_or(path);
    path.components()
        .any(|component| matches!(component, Component::Normal(name) if name == "tests"))
        || path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem == "tests" || stem.ends_with("_tests"))
}

fn is_exact_manifest_path(path: &Path, expected: &str) -> bool {
    manifest_relative(path).is_some_and(|path| path == Path::new(expected))
}

fn manifest_relative(path: &Path) -> Option<&Path> {
    if path.is_absolute() {
        path.strip_prefix(env!("CARGO_MANIFEST_DIR")).ok()
    } else {
        Some(path)
    }
}

pub(super) fn path_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| canonical_ident(&segment.ident))
        .collect::<Vec<_>>()
        .join("::")
}
