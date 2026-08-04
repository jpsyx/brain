//! Metadata a frontend provides for its hook integration.

/// Frontend-provided values needed to associate hooks with one launch.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct HookMetadata {
    values: Vec<(String, String)>,
}

impl std::fmt::Debug for HookMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookMetadata")
            .field(
                "keys",
                &self.values.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl HookMetadata {
    /// Construct hook metadata from frontend-owned key-value pairs.
    #[must_use]
    pub fn new(values: Vec<(String, String)>) -> Self {
        Self { values }
    }

    /// Construct metadata for a frontend with no hook integration.
    #[must_use]
    pub const fn none() -> Self {
        Self { values: Vec::new() }
    }

    /// The frontend-owned metadata entries.
    #[must_use]
    pub fn values(&self) -> &[(String, String)] {
        &self.values
    }
}
