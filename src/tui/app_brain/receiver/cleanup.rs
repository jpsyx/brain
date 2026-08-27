//! Exact local file cleanup for one receiver instance.

use crate::tui::App;

impl App {
    pub(super) fn cleanup_receiver_instance_files(&self, instance: &str) {
        let _ = self.cleanup_receiver_instance_files_checked(instance);
    }

    pub(super) fn cleanup_receiver_instance_files_checked(
        &self,
        instance: &str,
    ) -> std::io::Result<()> {
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
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}
