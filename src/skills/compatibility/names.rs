use std::collections::BTreeSet;

pub(super) fn expand(names: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut expanded: Vec<String> = names
        .iter()
        .filter(|name| seen.insert((*name).clone()))
        .cloned()
        .collect();
    for name in names {
        let successors: &[&str] = match name.as_str() {
            "second-brain" => &["projects", "process-capture", "organize", "brain-sync"],
            "todo" => &["agenda"],
            _ => &[],
        };
        for successor in successors {
            if seen.insert((*successor).to_owned()) {
                expanded.push((*successor).to_owned());
            }
        }
    }
    expanded
}
