use super::{catalog, markdown};

#[test]
fn second_brain_old_anchors_still_resolve() {
    for rendered in [false, true] {
        let documents = catalog::documents(rendered);
        let anchors = markdown::anchors(&documents["second-brain/SKILL.md"]);
        for anchor in [
            "second-brain",
            "personal-assistant-mode",
            "inbound-brain-messages-sms-and-email",
            "sending-a-longer-response-by-email",
            "core-principle",
            "intermediate-packets-ips",
            "the-four-buckets",
            "projects--outcome-driven-finite",
            "namespaces",
            "every-project-has-a-metadatajson--readmemd",
            "areas--ongoing-responsibilities",
            "resources--reference-and-curiosity",
            "archive--inactive-but-preserved",
            "capture--the-users-in-basket-not-a-para-bucket",
            "rules-for-capture",
            "a-file-is-not-a-note",
            "not-everything-in-capture-is-a-note",
            "process-capture--move-this-into-my-brain",
            "decision-flow",
            "naming-conventions",
            "referencing-markdown-files",
            "always-end-with-an-additions-table",
            "cross-link-new-notes-see-also",
            "end-of-session-clean-up-tool-byproducts",
            "commands",
            "working-with-tasks-linked-to-projects",
            "add-a-new-project--start-a-project-for-x",
            "mark-this-project-as-complete--mark-project-as-done--this-project-is-done",
            "archive-this-project--im-done-with-x",
            "add-a-resource--save-this-pdf-image-plaintext-notes",
            "paired-subdirectories-for-pdfs--media-with-notes",
            "get--fetch--find-items--search-and-retrieval",
            "extract-ip--extract-intermediate-packets-from-x",
            "reindex-the-second-brain--second-brain-reindex",
            "cloud-sync-the-brain--second-brain-cloud-sync",
            "resolve-sync-conflicts--second-brain-resolve-conflicts",
            "csv-tooling--keep-it-simple",
            "contacts",
            "when-something-doesnt-fit",
            "common-mistakes",
        ] {
            assert!(
                anchors.contains(anchor),
                "lost compatibility anchor {anchor}"
            );
        }
    }
}
#[test]
fn task_project_old_anchors_still_resolve() {
    for rendered in [false, true] {
        let documents = catalog::documents(rendered);
        let anchors = markdown::anchors(&documents["todo/references/task-project-link.md"]);
        for anchor in [
            "task--project-linkage",
            "golden-rule-no-sub-tasks-in-taskscsv",
            "forward-link",
            "reverse-link",
            "validation-cli-structured--never-llm",
            "personal-assistant-when-to-convert-a-task-to-a-project",
            "conversion-workflow-todo-turn-into-project-task",
            "other-linkage-commands",
            "archiving-a-project-extends-second-brain-archive-this-project",
            "reading-the-link-from-tasks--project",
            "reading-the-link-from-project--tasks",
        ] {
            assert!(
                anchors.contains(anchor),
                "lost compatibility anchor {anchor}"
            );
        }
    }
}

#[test]
fn adjacent_skills_keep_their_previous_entry_anchors() {
    for rendered in [false, true] {
        let documents = catalog::documents(rendered);
        let missing: Vec<_> = [
            ("brain-skills/SKILL.md", "create-a-skill"),
            (
                "todo/references/sync-rules.md",
                "order-of-operations-in-second-brain-reindex",
            ),
            (
                "contacts/SKILL.md",
                "lookup-precedence--local-csv-first-then-fallback",
            ),
            (
                "contacts/SKILL.md",
                "presenting-lookup-results--always-a-table",
            ),
            (
                "brain-knowledge-capture/SKILL.md",
                "personal-assistant-mode",
            ),
            (
                "brain-knowledge-capture/SKILL.md",
                "add-a-see-also-section-when-theres-something-worth-linking",
            ),
            (
                "article-summarizer/SKILL.md",
                "red-flags--stop-and-reconsider",
            ),
        ]
        .into_iter()
        .filter(|(path, anchor)| !markdown::anchors(&documents[*path]).contains(*anchor))
        .collect();
        assert!(
            missing.is_empty(),
            "lost compatibility anchors: {missing:?}"
        );
    }
}

#[test]
fn todo_agenda_and_triage_legacy_anchors_remain_resolvable() {
    for rendered in [false, true] {
        let documents = catalog::documents(rendered);
        for (path, expected) in [
            (
                "todo/SKILL.md",
                vec![
                    "first-agenda-preparation-collaborative-working-mode",
                    "updates-vs-regeneration--default-to-update",
                    "agenda-generation--phased-procedure",
                    "phase-1--load-state-read-only",
                    "phase-3--apply-daily-anchors-fixed-time-blocks",
                    "phase-10--work-hours-cutoff-optional",
                    "agenda-rules-and-constraints-daily-anchors",
                    "calendar-busy-blocks",
                    "work-hours-cutoff-optional-personal",
                    "surfacing-chunks-in-the-agenda--todo-today--todo-what",
                ],
            ),
            (
                "triage/SKILL.md",
                vec![
                    "skip-daily-triage-today--skip-the-morning-triage-habit-run-nothing-else",
                    "step-3--bulk-operations-per-group",
                    "step-9--mark-morning-triage-habit-done",
                    "1b--the-capture-in-basket",
                    "asking-the-user-for-per-task-actions-use-askuserquestion",
                ],
            ),
            (
                "todo/references/triage-heuristics.md",
                vec!["step-3--bulk-operations-per-group", "hard-deadline-tasks"],
            ),
            (
                "triage/references/heuristics.md",
                vec!["step-3--bulk-operations-per-group", "hard-deadline-tasks"],
            ),
        ] {
            let actual = markdown::anchors(&documents[path]);
            for anchor in expected {
                assert!(actual.contains(anchor), "{path} lost {anchor}");
            }
        }
    }
}
