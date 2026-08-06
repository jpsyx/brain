use crate::theme::Theme;

#[must_use]
pub fn setup_intro(theme: Theme) -> String {
    format!(
        "{}\n\nThis will enable cloud sync on this machine: brain will connect to an existing private Backblaze B2 bucket, verify the remote workspace identity, save the sync credentials in machine-local brain env, create the RCLONE_TEST safety marker, and establish the first baseline.\n",
        theme.accent("brain sync setup")
    )
}
