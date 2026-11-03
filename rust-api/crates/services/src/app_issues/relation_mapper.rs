#![forbid(unsafe_code)]

//! Issue-relation type mapping (`utils/issue_relation_mapper.py:1-32`).
//!
//! [`get_actual_relation`] (`:19-32`) is the stored row type for a
//! relation edge; [`get_inverse_relation`] (`:5-16`) is its mirror.
//! Both are thin wrappers over the merged
//! [`actual_relation`](crate::assistant::tools_issues::actual_relation) /
//! [`inverse_relation`](crate::assistant::tools_issues::inverse_relation)
//! ports — same four rewrite rules, same `.get(k, k)` passthrough —
//! never a parallel copy. D-12 already reuses those ports
//! (`orchestration::relations`); D-08 and D-18 consumers should reuse
//! this module (or `assistant::tools_issues` directly). The tiny
//! private `inverse_relation` in `tasks_webhooks::activity_misc` stays
//! as is (out of scope for this domain).
//!
//! Fixture: `rust-api/fixtures/app_issues/queries/FX-ISS-13.move.json`
//! (`get_actual_relation_truth_table`, `get_inverse_relation_truth_table`).
//! The `#[cfg(test)]` suite below replays both tables.

use crate::assistant::tools_issues::{actual_relation, inverse_relation};

/// The stored relation type (`get_actual_relation`, `:19-32`).
///
/// `start_after`/`finish_after`/`blocking`/`implements` rewrite to
/// their forward stored form; every other input (including the
/// identity entries `blocked_by`, `start_before`, `finish_before`,
/// `implemented_by` and any unknown string) passes through unchanged.
pub fn get_actual_relation(relation_type: &str) -> &str {
    actual_relation(relation_type)
}

/// The mirror relation type (`get_inverse_relation`, `:5-16`).
///
/// Unknown types map to themselves (`.get(k, k)`).
pub fn get_inverse_relation(relation_type: &str) -> &str {
    inverse_relation(relation_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Load the FX-ISS-13 fixture.
    fn fixture() -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/app_issues/queries/FX-ISS-13.move.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).expect("FX-ISS-13 fixture exists");
        serde_json::from_str(&text).expect("FX-ISS-13 fixture parses")
    }

    #[test]
    fn actual_relation_replays_truth_table() {
        let fx = fixture();
        let table = &fx["get_actual_relation_truth_table"];
        // Rewrite rules, pinned against the fixture strings.
        assert_eq!(table["start_after"].as_str().unwrap(), "start_before");
        assert_eq!(table["finish_after"].as_str().unwrap(), "finish_before");
        assert_eq!(table["blocking"].as_str().unwrap(), "blocked_by");
        assert_eq!(table["implements"].as_str().unwrap(), "implemented_by");
        for (input, expected) in [
            ("start_after", "start_before"),
            ("finish_after", "finish_before"),
            ("blocking", "blocked_by"),
            ("implements", "implemented_by"),
        ] {
            assert_eq!(get_actual_relation(input), expected, "{input}");
            assert_eq!(table[input].as_str().unwrap(), expected, "{input}");
        }
        // Identity + passthrough rows: the fixture annotates these
        // descriptively (`duplicate (passthrough — not in map)`), so the
        // test pins the behaviour itself.
        for input in [
            "blocked_by",
            "start_before",
            "finish_before",
            "implemented_by",
            "duplicate",
            "relates_to",
            "unknown_string",
            "",
        ] {
            assert_eq!(get_actual_relation(input), input, "{input}");
        }
    }

    #[test]
    fn inverse_relation_replays_truth_table() {
        let fx = fixture();
        let table = &fx["get_inverse_relation_truth_table"];
        for (input, expected) in [
            ("start_after", "start_before"),
            ("finish_after", "finish_before"),
            ("blocked_by", "blocking"),
            ("blocking", "blocked_by"),
            ("start_before", "start_after"),
            ("finish_before", "finish_after"),
            ("implemented_by", "implements"),
            ("implements", "implemented_by"),
        ] {
            assert_eq!(get_inverse_relation(input), expected, "{input}");
            assert_eq!(table[input].as_str().unwrap(), expected, "{input}");
        }
        for input in ["duplicate", "relates_to", "unknown", ""] {
            assert_eq!(get_inverse_relation(input), input, "{input}");
        }
    }
}
