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
        let cache_dir = self.context.workspace().paths().cache_dir();
        for relative in [
            std::path::PathBuf::from(format!("responses/{instance}.json")),
            std::path::PathBuf::from(format!("receiver-observations/{instance}.json")),
            std::path::PathBuf::from(format!("receiver-observations/{instance}.json.lock")),
        ] {
            crate::workspace::remove_regular_file_beneath(cache_dir, &relative)?;
        }
        Ok(())
    }
}
