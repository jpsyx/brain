use crate::sync::config::SyncConfig;

#[must_use]
pub(super) fn sync_block(
    bucket: &str,
    key_id: &str,
    app_key: &str,
    existing: &SyncConfig,
) -> serde_json::Value {
    serde_json::json!({
        "enabled": true,
        "b2_bucket": bucket,
        "b2_path": existing.b2_path,
        "b2_key_id": key_id,
        "b2_app_key": app_key,
        "crypt_password": existing.crypt_password,
        "crypt_password2": existing.crypt_password2,
        "crypt_filename_encryption": existing.crypt_filename_encryption,
        "crypt_directory_name_encryption": existing.crypt_directory_name_encryption,
    })
}
