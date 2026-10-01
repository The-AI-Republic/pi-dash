//! Cloud Agent structured output (D-11, stage 5).
//!
//! Port of `apps/api/pi_dash/cloud_agent/output.py:1-13`
//! (`CloudAgentOutput` + the `Evidence` / `Limitation` constrained strings).
//!
//! Translation notes:
//!
//! * Pydantic validates on every construction; the port validates in
//!   [`CloudAgentOutput::new`] and routes `Deserialize` through it, so both
//!   spellings enforce the same rules. Fields are private so no construction
//!   path can bypass validation.
//! * `strip_whitespace=True` is pydantic-core trim semantics: Unicode
//!   `White_Space` only, exactly `str::trim` — notably *not* Python
//!   `str.strip`, which would also strip `U+001C..=U+001F` (verified against
//!   the real `output.py` on pydantic 2.13.5: the separators survive).
//!   Stripping runs before the length check, as in pydantic. Note `summary`
//!   has a `max_length` but no strip (`output.py:11`) — ported as-is.
//! * Length caps count characters (code points), matching Python `len()` for
//!   every string representable in Rust (`&str` cannot hold the lone
//!   surrogates where the two counters could differ, and `serde_json`
//!   rejects those inputs before validation).
//! * `evidence` / `limitations` default to `[]` when absent but reject
//!   explicit `null` (pydantic `list` fields are not `Optional`); the
//!   `Deserialize` impl uses `#[serde(default)]` on a bare `Vec` to preserve
//!   the null-vs-absent distinction (Semantic traps). Unknown keys are
//!   ignored, as in pydantic v2's default.
//! * Serialization emits all four keys in field-definition order
//!   (`outcome, summary, evidence, limitations`), matching `model_dump()`.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-01-types.golden.json`
//! (`cloud_agent_output`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

/// `summary` cap (`output.py:11`).
pub const MAX_SUMMARY_CHARS: usize = 30_000;
/// `Evidence` item cap (`output.py:5`).
pub const MAX_EVIDENCE_CHARS: usize = 1_000;
/// `evidence` item-count cap (`output.py:12`).
pub const MAX_EVIDENCE_ITEMS: usize = 10;
/// `Limitation` item cap (`output.py:6`).
pub const MAX_LIMITATION_CHARS: usize = 500;
/// `limitations` item-count cap (`output.py:13`).
pub const MAX_LIMITATION_ITEMS: usize = 10;

/// `outcome` values (`output.py:10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Completed,
    Blocked,
    Noop,
}

/// Constraint failure: the `pydantic.ValidationError` analog for
/// [`CloudAgentOutput`] construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputValidationError {
    field: &'static str,
    message: String,
}

impl OutputValidationError {
    /// The offending field (`outcome`, `summary`, `evidence`, `limitations`).
    pub fn field(&self) -> &'static str {
        self.field
    }
}

impl std::fmt::Display for OutputValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for OutputValidationError {}

/// pydantic-core `strip_whitespace` semantics: Unicode `White_Space` only,
/// exactly `str::trim` (`U+001C..=U+001F` survive — the real `output.py`
/// keeps them on pydantic 2.13.5).
fn pydantic_strip(text: &str) -> &str {
    text.trim()
}

/// Structured agent outcome (`output.py:9-13`).
///
/// Field order is the `model_dump()` wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudAgentOutput {
    outcome: Outcome,
    summary: String,
    evidence: Vec<String>,
    limitations: Vec<String>,
}

impl CloudAgentOutput {
    /// Validated construction (the `CloudAgentOutput(...)` call).
    ///
    /// `evidence` / `limitations` items are stripped before the length check;
    /// the stored values are the stripped ones, as in pydantic.
    pub fn new(
        outcome: Outcome,
        summary: String,
        evidence: Vec<String>,
        limitations: Vec<String>,
    ) -> Result<Self, OutputValidationError> {
        if summary.chars().count() > MAX_SUMMARY_CHARS {
            return Err(OutputValidationError {
                field: "summary",
                message: format!("at most {MAX_SUMMARY_CHARS} characters"),
            });
        }
        let evidence =
            Self::validate_items("evidence", evidence, MAX_EVIDENCE_ITEMS, MAX_EVIDENCE_CHARS)?;
        let limitations = Self::validate_items(
            "limitations",
            limitations,
            MAX_LIMITATION_ITEMS,
            MAX_LIMITATION_CHARS,
        )?;
        Ok(CloudAgentOutput {
            outcome,
            summary,
            evidence,
            limitations,
        })
    }

    fn validate_items(
        field: &'static str,
        items: Vec<String>,
        max_items: usize,
        max_chars: usize,
    ) -> Result<Vec<String>, OutputValidationError> {
        if items.len() > max_items {
            return Err(OutputValidationError {
                field,
                message: format!("at most {max_items} items"),
            });
        }
        items
            .into_iter()
            .map(|item| {
                let stripped = pydantic_strip(&item);
                if stripped.chars().count() > max_chars {
                    return Err(OutputValidationError {
                        field,
                        message: format!("item over {max_chars} characters"),
                    });
                }
                Ok(stripped.to_string())
            })
            .collect()
    }

    pub fn outcome(&self) -> Outcome {
        self.outcome
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn evidence(&self) -> &[String] {
        &self.evidence
    }

    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }
}

/// Raw parse shape. `outcome` / `summary` are required (`None` covers both
/// absent and `null`, and both are errors). The defaulted lists use
/// `#[serde(default)]` on a bare `Vec`: absent → `[]`, explicit `null` → a
/// type error — which is exactly the null-vs-absent distinction pydantic
/// draws for non-`Optional` list fields.
#[derive(Deserialize)]
struct RawOutput {
    outcome: Option<Outcome>,
    summary: Option<String>,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    limitations: Vec<String>,
}

impl<'de> Deserialize<'de> for CloudAgentOutput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawOutput::deserialize(deserializer)?;
        let outcome = raw
            .outcome
            .ok_or_else(|| D::Error::missing_field("outcome"))?;
        let summary = raw
            .summary
            .ok_or_else(|| D::Error::missing_field("summary"))?;
        CloudAgentOutput::new(outcome, summary, raw.evidence, raw.limitations)
            .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-01-types.golden.json");

    fn output_fixture() -> Value {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        fixture["cloud_agent_output"].clone()
    }

    #[test]
    fn example_dump_serializes_byte_exact() {
        let golden = &output_fixture()["example_dump"];
        let output = CloudAgentOutput::new(
            Outcome::Completed,
            "did it".into(),
            vec!["e1".into()],
            vec!["l1".into()],
        )
        .expect("valid");
        // String comparison pins key order as well as content.
        assert_eq!(
            serde_json::to_string(&output).expect("serializes"),
            serde_json::to_string(golden).expect("golden serializes")
        );
        assert_eq!(
            serde_json::to_string(&output).expect("serializes"),
            r#"{"outcome":"completed","summary":"did it","evidence":["e1"],"limitations":["l1"]}"#
        );
    }

    #[test]
    fn schema_golden_matches_fixture() {
        let golden = output_fixture();
        assert_eq!(
            golden["outcome_enum"]["enum"],
            json!(["completed", "blocked", "noop"])
        );
        assert_eq!(golden["summary"]["maxLength"], MAX_SUMMARY_CHARS);
        assert_eq!(golden["evidence"]["items"]["maxLength"], MAX_EVIDENCE_CHARS);
        assert_eq!(golden["evidence"]["maxItems"], MAX_EVIDENCE_ITEMS);
        assert_eq!(
            golden["limitations"]["items"]["maxLength"],
            MAX_LIMITATION_CHARS
        );
        assert_eq!(golden["limitations"]["maxItems"], MAX_LIMITATION_ITEMS);
        assert_eq!(golden["required"], json!(["outcome", "summary"]));
    }

    #[test]
    fn constraint_probes_all_fail() {
        let probes = &output_fixture()["constraint_probes"];
        for key in [
            "bad_outcome",
            "evidence_11",
            "evidence_1001_chars",
            "limitations_501",
            "summary_30001",
        ] {
            assert_eq!(probes[key], "ValidationError", "probe {key}");
        }

        let bad_outcome = json!({"outcome": "exploded", "summary": "s"});
        assert!(serde_json::from_value::<CloudAgentOutput>(bad_outcome).is_err());

        let evidence_11 = json!({"outcome": "noop", "summary": "s", "evidence": vec!["e"; 11]});
        let err = serde_json::from_value::<CloudAgentOutput>(evidence_11).expect_err("11 items");
        assert!(err.to_string().contains("evidence"), "{err}");

        let item_1001 = "e".repeat(1001);
        let too_long = json!({"outcome": "noop", "summary": "s", "evidence": [item_1001]});
        assert!(serde_json::from_value::<CloudAgentOutput>(too_long).is_err());

        let item_501 = "l".repeat(501);
        let too_long = json!({"outcome": "noop", "summary": "s", "limitations": [item_501]});
        assert!(serde_json::from_value::<CloudAgentOutput>(too_long).is_err());

        let summary_30001 = "s".repeat(30_001);
        let too_long = json!({"outcome": "noop", "summary": summary_30001});
        let err = serde_json::from_value::<CloudAgentOutput>(too_long).expect_err("long summary");
        assert!(err.to_string().contains("summary"), "{err}");

        // Boundaries hold: exactly-at-cap parses.
        let summary_30000 = "s".repeat(30_000);
        let evidence_1000 = "e".repeat(1000);
        let limitation_500 = "l".repeat(500);
        let at_cap = json!({
            "outcome": "blocked",
            "summary": summary_30000,
            "evidence": [evidence_1000],
            "limitations": [limitation_500],
        });
        assert!(serde_json::from_value::<CloudAgentOutput>(at_cap).is_ok());
    }

    #[test]
    fn null_vs_absent_preserved() {
        // Absent lists default to [] (required: outcome, summary only).
        let minimal = json!({"outcome": "completed", "summary": "Done"});
        let output: CloudAgentOutput = serde_json::from_value(minimal).expect("minimal parses");
        assert!(output.evidence().is_empty());
        assert!(output.limitations().is_empty());
        // ... and serialize back as [] (model_dump shape).
        assert_eq!(
            serde_json::to_value(&output).expect("serializes"),
            json!({"outcome": "completed", "summary": "Done", "evidence": [], "limitations": []})
        );

        // Explicit null is rejected everywhere (fields are not Optional).
        for body in [
            json!({"outcome": null, "summary": "s"}),
            json!({"outcome": "noop", "summary": null}),
            json!({"outcome": "noop", "summary": "s", "evidence": null}),
            json!({"outcome": "noop", "summary": "s", "limitations": null}),
        ] {
            assert!(
                serde_json::from_value::<CloudAgentOutput>(body.clone()).is_err(),
                "null rejected: {body}"
            );
        }

        // Missing required fields are rejected.
        assert!(serde_json::from_value::<CloudAgentOutput>(json!({"summary": "s"})).is_err());
        assert!(serde_json::from_value::<CloudAgentOutput>(json!({"outcome": "noop"})).is_err());

        // Unknown keys are ignored (pydantic v2 default).
        let extra = json!({"outcome": "noop", "summary": "s", "bogus": 1});
        assert!(serde_json::from_value::<CloudAgentOutput>(extra).is_ok());
    }

    #[test]
    fn evidence_and_limitations_strip_before_length_check() {
        // Stripped values are what get stored.
        let output = CloudAgentOutput::new(
            Outcome::Noop,
            "s".into(),
            vec!["  padded\t".into()],
            vec!["\nline\n".into()],
        )
        .expect("valid");
        assert_eq!(output.evidence(), &["padded".to_string()]);
        assert_eq!(output.limitations(), &["line".to_string()]);

        // Length is counted after stripping: 1000 chars plus padding is OK.
        let padded = format!("  {}  ", "e".repeat(1000));
        let output = CloudAgentOutput::new(Outcome::Noop, "s".into(), vec![padded], vec![])
            .expect("strip-then-count");
        assert_eq!(output.evidence()[0].chars().count(), 1000);

        // U+001C..=U+001F are NOT stripped: pydantic-core trims White_Space
        // only (observed on the real output.py under pydantic 2.13.5), even
        // though Python str.strip would remove them.
        let output = CloudAgentOutput::new(
            Outcome::Noop,
            "s".into(),
            vec!["\u{1c}sep\u{1d}".into()],
            vec![],
        )
        .expect("valid");
        assert_eq!(output.evidence(), &["\u{1c}sep\u{1d}".to_string()]);

        // Non-ASCII White_Space (NEL, NBSP) still strips.
        let output = CloudAgentOutput::new(
            Outcome::Noop,
            "s".into(),
            vec!["\u{85}pad\u{a0}".into()],
            vec![],
        )
        .expect("valid");
        assert_eq!(output.evidence(), &["pad".to_string()]);

        // summary is NOT stripped (output.py:11 has max_length only).
        let output =
            CloudAgentOutput::new(Outcome::Noop, "  kept  ".into(), vec![], vec![]).expect("valid");
        assert_eq!(output.summary(), "  kept  ");
    }
}
