//! Honest per-frontend capability enforcement levels.

/// What one frontend launch actually enforces for a requested capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityEnforcement {
    /// Launch configuration excludes nonselected frontend/global sources.
    StrictlySelected,
    /// The selected set is trusted guidance, but other sources may remain.
    AdvisoryOnly,
    /// Required machine-local material is absent or invalid.
    Unavailable,
}

/// Facts proven by one frontend's concrete launch configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnforcementEvidence {
    pub(super) strict_mcps: bool,
    pub(super) strict_skills: bool,
}

impl EnforcementEvidence {
    /// No per-launch exclusion of global sources was proven.
    #[must_use]
    pub const fn advisory_only() -> Self {
        Self {
            strict_mcps: false,
            strict_skills: false,
        }
    }

    /// MCP launch arguments prove selection, while skills remain advisory.
    #[must_use]
    pub const fn strict_mcps_only() -> Self {
        Self {
            strict_mcps: true,
            strict_skills: false,
        }
    }
}

/// Enforcement levels for all requested capabilities of one kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityEnforcementSet {
    pub(super) entries: Vec<(String, CapabilityEnforcement)>,
}

impl CapabilityEnforcementSet {
    /// Enforcement level for one requested logical name.
    #[must_use]
    pub fn enforcement(&self, name: &str) -> Option<CapabilityEnforcement> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == name)
            .map(|(_, enforcement)| *enforcement)
    }
}

/// Honest frontend enforcement report, separate from logical resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityEnforcementReport {
    pub mcps: CapabilityEnforcementSet,
    pub skills: CapabilityEnforcementSet,
}
