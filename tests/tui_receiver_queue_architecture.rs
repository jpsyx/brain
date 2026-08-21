use std::path::Path;

#[test]
fn inbound_queue_representation_and_mutation_stay_inside_queue_module() {
    let tui_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let queue_path = tui_root.join("receiver/queue.rs");
    let mut leaks = Vec::new();

    for entry in walkdir::WalkDir::new(&tui_root) {
        let entry = entry.expect("walk TUI source");
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || path == queue_path
        {
            continue;
        }

        let source = std::fs::read_to_string(path).expect("read TUI source");
        for forbidden in [
            "Vec<InboundJob>",
            "Vec<crate::server::receiver::InboundJob>",
            "queue.push(",
            "queue.pop(",
            "queue[",
            "queue.remove(0)",
            "queue.split_off(",
            "receiver_queue.push(",
            "receiver_queue.pop(",
            "receiver_queue[",
            "receiver_queue.remove(0)",
            "receiver_queue.split_off(",
        ] {
            if source.contains(forbidden) {
                leaks.push(format!("{}: {forbidden}", path.display()));
            }
        }
    }

    assert!(
        leaks.is_empty(),
        "inbound queue representation or mutation leaked outside receiver/queue.rs:\n{}",
        leaks.join("\n")
    );
}
