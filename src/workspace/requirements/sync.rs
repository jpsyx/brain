use super::{FeatureStatus, PromptMetadata};
use crate::sync::config::SyncConfigInspection;

pub(super) fn statuses(value: Option<&serde_json::Value>) -> (FeatureStatus, FeatureStatus) {
    match crate::sync::config::SyncConfig::inspect_value(value) {
        SyncConfigInspection::Off => (FeatureStatus::Off, FeatureStatus::Off),
        SyncConfigInspection::Ready(config) => (
            FeatureStatus::Ready,
            if config.watch_effective() {
                FeatureStatus::Ready
            } else {
                FeatureStatus::Off
            },
        ),
        SyncConfigInspection::Incomplete => {
            let watcher = value
                .and_then(|value| value.get("watch"))
                .and_then(serde_json::Value::as_bool)
                .map_or(FeatureStatus::Off, |watch| {
                    if watch {
                        FeatureStatus::Incomplete
                    } else {
                        FeatureStatus::Off
                    }
                });
            (FeatureStatus::Incomplete, watcher)
        }
    }
}

pub(super) fn prompts() -> Vec<PromptMetadata> {
    vec![
        PromptMetadata::plain("B2 bucket"),
        PromptMetadata::plain("B2 path"),
        PromptMetadata::secret("B2 key ID"),
        PromptMetadata::secret("B2 application key"),
    ]
}
