/// What the file sync actually moved, as one plain-language line. Pure.
///
/// A step name alone ("Starting rclone sync…") leaves a user watching a long
/// pause with no idea whether anything is happening or whether there was simply
/// nothing to do. Naming the finding distinguishes the two.
#[must_use]
pub fn format_file_findings(run: &crate::sync::run::RunOutcome) -> String {
    if !run.exit_ok {
        return format!(
            "  found: rclone reported {} error(s) → the run is not clean",
            run.errors
        );
    }
    if run.transferred == 0 && run.deleted == 0 {
        return "  found: no files differed between this machine and the remote".to_owned();
    }
    let mut parts = Vec::new();
    if run.transferred > 0 {
        parts.push(format!("{} file(s) transferred", run.transferred));
    }
    if run.deleted > 0 {
        parts.push(format!("{} file(s) deleted", run.deleted));
    }
    format!("  found: {}", parts.join(", "))
}
