//! Mint a fresh runner UUID locally.
//!
//! Used by both ``pidash runner add`` and the TUI add-runner form so the
//! cloud and local config agree on the runner_id from the very first
//! message. Cloud trusts the daemon-supplied UUID — if a user accidentally
//! collides (vanishingly unlikely with v4), the cloud rejects the create
//! with a 409 and the caller surfaces it as a normal error.

use uuid::Uuid;

/// Mint a fresh v4 UUID for a new runner. Thin wrapper so CLI and TUI
/// share the same call site (and a future change — e.g. namespaced UUIDs
/// per host — only needs to update one place).
pub fn mint() -> Uuid {
    Uuid::new_v4()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_is_unique() {
        let a = mint();
        let b = mint();
        assert_ne!(a, b);
    }
}
