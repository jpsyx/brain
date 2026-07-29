//! Machine-local provider configuration with process-environment compatibility.

fn prefer_process_value(process: Option<String>, stored: Option<String>) -> Option<String> {
    process
        .filter(|value| !value.trim().is_empty())
        .or(stored)
}

pub(super) fn get(process_name: &str, stored_name: &str) -> Option<String> {
    prefer_process_value(
        std::env::var(process_name).ok(),
        crate::env::get(stored_name),
    )
}

#[cfg(test)]
mod tests {
    use super::prefer_process_value;

    #[test]
    fn nonempty_process_value_overrides_machine_local_value() {
        assert_eq!(
            prefer_process_value(Some("process".to_owned()), Some("stored".to_owned())),
            Some("process".to_owned())
        );
    }

    #[test]
    fn blank_process_value_falls_back_to_machine_local_value() {
        assert_eq!(
            prefer_process_value(Some("  ".to_owned()), Some("stored".to_owned())),
            Some("stored".to_owned())
        );
    }
}
