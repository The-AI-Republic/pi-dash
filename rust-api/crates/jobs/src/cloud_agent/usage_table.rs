//! Usage-table loader (`fixtures/cloud_agent/usage-providers.golden.json`).
//!
//! The chat engine reads only the `usage_providers` section at boot
//! (agent naming convention P2, section 4.4); the `usage_vectors`
//! section backs the usage replay tests. [`load`] requires exactly
//! those two sections.

use serde_json::Value;

/// The cloud-agent usage fixture: `usage_providers` (run time) and
/// `usage_vectors` (usage replay tests).
const USAGE_GOLDEN: &str =
    include_str!("../../../../fixtures/cloud_agent/usage-providers.golden.json");

/// Usage-table load failure: the embedded fixture is corrupt or has
/// drifted from the shape the engine expects. Fail-closed at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageTableError(pub String);

impl std::fmt::Display for UsageTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "usage table invalid: {}", self.0)
    }
}

impl std::error::Error for UsageTableError {}

/// The loaded usage fixture document ([`load`]).
#[derive(Debug, Clone)]
pub struct UsageDocument {
    document: Value,
}

impl UsageDocument {
    /// The provider rows (`usage_providers`).
    pub fn usage_providers(&self) -> Result<&Vec<Value>, UsageTableError> {
        self.document
            .get("usage_providers")
            .and_then(Value::as_array)
            .ok_or_else(|| UsageTableError("missing `usage_providers`".to_string()))
    }

    /// The replay vectors (`usage_vectors`).
    pub fn usage_vectors(&self) -> Result<&Vec<Value>, UsageTableError> {
        self.document
            .get("usage_vectors")
            .and_then(Value::as_array)
            .ok_or_else(|| UsageTableError("missing `usage_vectors`".to_string()))
    }
}

/// Parse and validate the cloud-agent usage fixture. Requires exactly
/// the two sections the engine and its tests use; anything else is
/// drift and fails closed.
pub fn load() -> Result<UsageDocument, UsageTableError> {
    let document: Value =
        serde_json::from_str(USAGE_GOLDEN).map_err(|err| UsageTableError(err.to_string()))?;
    let doc = UsageDocument { document };
    let _ = doc.usage_providers()?;
    let _ = doc.usage_vectors()?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_fixture_loads_with_both_sections() {
        let doc = load().expect("usage fixture parses");
        assert_eq!(doc.usage_providers().expect("providers").len(), 43);
        assert_eq!(doc.usage_vectors().expect("vectors").len(), 66);
    }

}
