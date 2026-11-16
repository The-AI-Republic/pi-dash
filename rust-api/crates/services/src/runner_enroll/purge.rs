#![forbid(unsafe_code)]

//! `purge_local` query-flag parsing (services-B, PIDASHCONV-587).
//!
//! Port of `parse_purge_local` (`runner/services/runner_delete.py:146-163`):
//! strict bool semantics, empty/missing falls back to `default` (every
//! production caller passes the `True` default), anything else raises.
//!
//! The `delete_*` functions in the same Python file belong to services-C
//! (PIDASHCONV-588); this unit lives in its own file so the two issues
//! never touch the same path.
//!
//! Fixture: `rust-api/fixtures/runner_enroll/services/flows.golden.json`
//! (`parse_purge_local`: rule + note + all 13 golden cases). The
//! `#[cfg(test)]` suite replays every case from the fixture.
//!
//! Ported bugs: none found in this unit on read-through.

/// The `ValueError` message (`runner_delete.py:161-163`).
///
/// Callers catch it into a 400 `{"error": str(exc)}` (fixture note); the
/// status/body rendering stays the handler's job.
pub const PURGE_LOCAL_ERROR: &str = "purge_local must be one of: true, false, 1, 0, yes, no";

/// `parse_purge_local` rejection — the `ValueError` above, typed.
///
/// `Display` renders exactly [`PURGE_LOCAL_ERROR`], mirroring
/// `str(exc)` in the callers' 400 bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{PURGE_LOCAL_ERROR}")]
pub struct PurgeFlagError;

/// Parse a `purge_local` query-string flag (`runner_delete.py:146-163`).
///
/// `raw` is the `QueryDict.get("purge_local")` value — the *last* value on
/// repeats, `None` when absent — so multi-value extraction stays the
/// caller's job, like the `params.rs` precedent.
///
/// Semantics, in Python order (`(get(...) or "").strip().lower()`):
/// empty/missing yields `default`; `"true"`/`"1"`/`"yes"` yield true and
/// `"false"`/`"0"`/`"no"` yield false, case-insensitively with surrounding
/// whitespace stripped; anything else is [`PurgeFlagError`]. `str.strip`
/// and `str.lower` are Unicode-aware on both sides, so exotic whitespace
/// and casing fold identically.
pub fn parse_purge_local(raw: Option<&str>, default: bool) -> Result<bool, PurgeFlagError> {
    let normalized = raw.unwrap_or("").trim().to_lowercase();
    if normalized.is_empty() {
        return Ok(default);
    }
    match normalized.as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(PurgeFlagError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The F6 golden file, loaded verbatim (sibling `include_str!`
    /// precedent: `runner_runs/guards.rs`).
    fn fixture() -> serde_json::Value {
        let text = include_str!("../../../../fixtures/runner_enroll/services/flows.golden.json");
        serde_json::from_str(text).expect("flows.golden.json parses")
    }

    #[test]
    fn error_message_matches_source_and_fixture() {
        assert_eq!(
            PURGE_LOCAL_ERROR,
            "purge_local must be one of: true, false, 1, 0, yes, no"
        );
        assert_eq!(PurgeFlagError.to_string(), PURGE_LOCAL_ERROR);
        let goldens = &fixture()["parse_purge_local"]["cases"];
        for case in goldens.as_array().expect("cases is an array") {
            if let Some(expected) = case.get("error").and_then(|e| e.as_str()) {
                assert_eq!(expected, PURGE_LOCAL_ERROR, "case {case}");
            }
        }
    }

    #[test]
    fn replays_all_f6_purge_goldens() {
        let goldens = &fixture()["parse_purge_local"]["cases"];
        let cases = goldens.as_array().expect("cases is an array");
        assert_eq!(cases.len(), 13, "fixture case count pinned");
        for case in cases {
            let raw = case.get("raw").and_then(|r| r.as_str());
            let default = case
                .get("default")
                .and_then(|d| d.as_bool())
                .unwrap_or(true);
            let result = parse_purge_local(raw, default);
            if case.get("raises").is_some() {
                assert_eq!(result, Err(PurgeFlagError), "case {case}");
            } else {
                let expected = case["result"].as_bool().expect("result bool");
                assert_eq!(result, Ok(expected), "case {case}");
            }
        }
    }
}
