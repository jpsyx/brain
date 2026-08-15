use std::fmt::{Display, Formatter};

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    pub(super) const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self> {
        let core = value
            .trim()
            .strip_prefix('v')
            .unwrap_or_else(|| value.trim())
            .split(['-', '+'])
            .next()
            .unwrap_or_default();
        let mut components = core.split('.');
        let major = parse_component(components.next(), value)?;
        let minor = parse_component(components.next(), value)?;
        let patch = parse_component(components.next(), value)?;
        if components.next().is_some() {
            return Err(anyhow!("invalid Brain version {value:?}"));
        }
        Ok(Self::new(major, minor, patch))
    }
}

fn parse_component(component: Option<&str>, value: &str) -> Result<u64> {
    component
        .filter(|component| !component.is_empty())
        .ok_or_else(|| anyhow!("invalid Brain version {value:?}"))?
        .parse()
        .map_err(|_| anyhow!("invalid Brain version {value:?}"))
}

impl Display for Version {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
