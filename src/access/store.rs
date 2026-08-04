use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::AccessMode;

pub(crate) fn ensure_portable_access_mode(root: &Path, default: AccessMode) -> Result<()> {
    let path = root.join(".config/config.json");
    let mut config = crate::settings::load_map_at(&path);
    config
        .entry("access_mode".to_owned())
        .or_insert_with(|| Value::String(default.as_config_value().to_owned()));
    crate::settings::save_map_at(&path, &config)
}
