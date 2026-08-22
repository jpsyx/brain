use super::TasksState;
use crate::tasks::task::Task;
use crate::tui::links::{Link, LinkKind, extract_urls};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TaskLinksPlan {
    None,
    Open { url: String },
    Choose { task_id: String, links: Vec<Link> },
}

impl TasksState {
    pub(crate) fn selected_link_kind(&self, linear_base: &str) -> LinkKind {
        let Some(task) = self.selected_task() else {
            return LinkKind::None;
        };
        classify(task, &links_for(task, linear_base))
    }

    pub(crate) fn selected_links_plan(&self, linear_base: &str) -> TaskLinksPlan {
        let Some(task) = self.selected_task() else {
            return TaskLinksPlan::None;
        };
        let links = links_for(task, linear_base);
        match links.as_slice() {
            [] => TaskLinksPlan::None,
            [link] => TaskLinksPlan::Open {
                url: link.url.clone(),
            },
            _ => TaskLinksPlan::Choose {
                task_id: task.id.clone(),
                links,
            },
        }
    }
}

fn classify(task: &Task, links: &[Link]) -> LinkKind {
    match links.len() {
        0 => LinkKind::None,
        1 if task.has_linear() => LinkKind::SingleLinear,
        1 => LinkKind::SingleNotes,
        _ => LinkKind::Multiple,
    }
}

fn links_for(task: &Task, linear_base: &str) -> Vec<Link> {
    let mut links = Vec::new();
    if let Some(url) = task.linear_url(linear_base) {
        links.push(Link {
            label: format!("Linear {}", task.linear_issue.trim()),
            url,
        });
    }
    let detail_urls = extract_urls(&task.see_also)
        .into_iter()
        .chain(extract_urls(&task.notes));
    for url in detail_urls {
        if links.iter().any(|link| link.url == url) {
            continue;
        }
        links.push(Link {
            label: url.clone(),
            url,
        });
    }
    links
}

#[cfg(test)]
mod tests {
    use super::{classify, links_for};
    use crate::tasks::task::test_task;
    use crate::tui::links::LinkKind;

    const LINEAR_BASE: &str = "https://linear.example/issue/";

    #[test]
    fn destinations_are_ordered_and_deduplicated_inside_task_state() {
        let mut task = test_task("T9", "not_started");
        task.linear_issue = "OPS-123".to_owned();
        task.see_also = "https://reference.example/spec".to_owned();
        task.notes = concat!(
            "https://linear.example/issue/OPS-123 ",
            "https://reference.example/design"
        )
        .to_owned();

        let links = links_for(&task, LINEAR_BASE);

        assert_eq!(
            links
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            [
                "https://linear.example/issue/OPS-123",
                "https://reference.example/spec",
                "https://reference.example/design"
            ]
        );
        assert_eq!(links[0].label, "Linear OPS-123");
    }

    #[test]
    fn classification_distinguishes_each_palette_label_policy() {
        let plain = test_task("T1", "not_started");
        assert_eq!(
            classify(&plain, &links_for(&plain, LINEAR_BASE)),
            LinkKind::None
        );

        let mut linear = test_task("T2", "not_started");
        linear.linear_issue = "OPS-2".to_owned();
        assert_eq!(
            classify(&linear, &links_for(&linear, LINEAR_BASE)),
            LinkKind::SingleLinear
        );

        let mut notes = test_task("T3", "not_started");
        notes.notes = "https://reference.example/only".to_owned();
        assert_eq!(
            classify(&notes, &links_for(&notes, LINEAR_BASE)),
            LinkKind::SingleNotes
        );

        linear.notes = "https://reference.example/extra".to_owned();
        assert_eq!(
            classify(&linear, &links_for(&linear, LINEAR_BASE)),
            LinkKind::Multiple
        );
    }
}
