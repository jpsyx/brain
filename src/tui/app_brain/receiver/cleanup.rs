//! Exact local file cleanup for one receiver instance.

use crate::tui::App;

impl App {
    pub(super) fn cleanup_receiver_instance_files(&self, instance: &str) {
        let response = self
            .context
            .workspace()
            .paths()
            .responses_dir()
            .join(format!("{instance}.json"));
        let observation = self.receiver_observation_path(instance);
        for path in [
            &response,
            &observation,
            &observation.with_extension("json.lock"),
        ] {
            let _ = std::fs::remove_file(path);
        }
    }
}
