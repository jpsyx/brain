//! Secret-free configuration validation and UUID-pinned workspace resolution.
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::tasks::removal::model::{
    AdapterSnapshot, IntegrationPolicy, IntegrationSnapshot, Problem,
};
use crate::workspace::{CommandContext, RegistryStore};

pub(crate) const POLICY: &str = "task_removal_integration";
pub(crate) const ADAPTERS: &str = "task_removal_adapters";

pub(crate) fn deserialize_policy<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<IntegrationPolicy>, D::Error> {
    let policy = Option::<IntegrationPolicy>::deserialize(deserializer)?;
    if let Some(policy) = &policy {
        validate_policy(policy).map_err(serde::de::Error::custom)?;
    }
    Ok(policy)
}

pub(crate) fn parse_policy(value: Value) -> anyhow::Result<Option<IntegrationPolicy>> {
    let policy: Option<IntegrationPolicy> = serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("invalid task removal policy object"))?;
    if let Some(policy) = &policy {
        validate_policy(policy)?;
    }
    Ok(policy)
}

fn validate_policy(policy: &IntegrationPolicy) -> anyhow::Result<()> {
    anyhow::ensure!(
        !policy.adapter.trim().is_empty(),
        "adapter name must not be empty"
    );
    anyhow::ensure!(
        policy.link_field == "linear_issue",
        "link_field must be linear_issue"
    );
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterConfig {
    argv: Vec<String>,
    env_names: Vec<String>,
    timeout_seconds: u64,
}

pub(crate) fn parse_adapters(value: Value) -> anyhow::Result<BTreeMap<String, AdapterSnapshot>> {
    let configs: BTreeMap<String, AdapterConfig> = serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("invalid task removal adapters object"))?;
    configs
        .into_iter()
        .map(|(name, config)| {
            anyhow::ensure!(!name.trim().is_empty(), "adapter name must not be empty");
            anyhow::ensure!(
                config
                    .argv
                    .first()
                    .is_some_and(|path| Path::new(path).is_absolute()),
                "adapter argv requires an absolute executable path"
            );
            anyhow::ensure!(
                config.argv.iter().all(|arg| !arg.contains('\0')),
                "adapter argv must not contain NUL"
            );
            anyhow::ensure!(
                (1..=120).contains(&config.timeout_seconds),
                "adapter timeout_seconds must be 1-120"
            );
            let names: BTreeSet<_> = config.env_names.iter().cloned().collect();
            anyhow::ensure!(
                names.len() == config.env_names.len()
                    && names.iter().all(|name| valid_environment_name(name)),
                "adapter env_names must be unique environment names"
            );
            Ok((
                name,
                AdapterSnapshot {
                    argv: config.argv,
                    environment_names: names,
                    timeout_ms: config.timeout_seconds * 1000,
                },
            ))
        })
        .collect()
}

fn valid_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub(crate) fn load(context: &CommandContext) -> Result<IntegrationSnapshot, Problem> {
    let config = crate::config::Config::try_load(&context.workspace).map_err(|_| invalid())?;
    let Some(policy) = config.task_removal_integration else {
        return Ok(IntegrationSnapshot::default());
    };
    let registry =
        RegistryStore::load_from(context.registry_store.path()).map_err(|_| invalid())?;
    let selected = registry
        .select(Some(context.workspace.name().as_str()))
        .map_err(|_| invalid())?;
    if selected.record().workspace_id != context.workspace.id() {
        return Err(invalid());
    }
    let adapters = parse_adapters(
        selected
            .record()
            .env
            .get(ADAPTERS)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|_| invalid())?;
    let adapter = adapters
        .get(&policy.adapter)
        .filter(|adapter| executable_available(&adapter.argv[0]))
        .cloned();
    Ok(IntegrationSnapshot {
        policy: Some(policy),
        adapter,
    })
}

fn executable_available(path: &str) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn invalid() -> Problem {
    Problem::new(
        "invalid_removal_integration",
        "Task removal integration configuration or workspace provenance is invalid",
        serde_json::json!({}),
    )
}
