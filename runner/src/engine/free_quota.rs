//! OpenHub free-lane quota classification (issue PDASHOSS01-363).
//!
//! On the `openhub` lane the managed-runner engine calls the gateway's
//! Responses API directly. With the daily free token allowance the gateway
//! answers with structured error bodies instead of a generic failure:
//!
//! | Status | `code` | Disposition |
//! | ------ | ------ | ----------- |
//! | 402 | `free_quota_exhausted` (body: `limit`, `plan`, `resets_at`) | Stop: daily free limit reached, keeping `resets_at` |
//! | 402 | `free_plan_paid_lane_blocked` | Stop: this model needs credit |
//! | 402 | `insufficient_balance` / `no_credit_account` | Stop: wallet empty, top up |
//! | 429 | `free_lane_busy` | Retry with backoff; never fail the run |
//! | 503 | `free_quota_unavailable` | Retry with backoff; never fail the run |
//!
//! The classifier is keyed on the lane: only `openhub` classifies, so every
//! other lane keeps today's behaviour. It reads the `code` field (an
//! OpenAI-style `error.code`, or a top-level `code`) out of the gateway body
//! text the engine forwards; bodies without a recognised code classify as
//! `None` and the caller falls back to its existing handling.
//!
//! A `Retry` means the caller restarts the turn after [`backoff`]: the
//! engine's own ~200ms retries cannot clear a busy lane and end in a
//! generic failure, so the runner drives the retry instead.

use std::time::Duration;

/// The gateway lane this classification applies to. Every other lane is
/// unaffected.
pub const OPENHUB_LANE: &str = "openhub";

/// Error codes that stop the run with a clear reason.
pub const CODE_EXHAUSTED: &str = "free_quota_exhausted";
pub const CODE_NEEDS_CREDIT: &str = "free_plan_paid_lane_blocked";
pub const CODE_WALLET_EMPTY: &str = "insufficient_balance";
pub const CODE_NO_CREDIT_ACCOUNT: &str = "no_credit_account";

/// Error codes that retry with backoff and never fail the run.
pub const CODE_BUSY: &str = "free_lane_busy";
pub const CODE_UNAVAILABLE: &str = "free_quota_unavailable";

/// How many times a run restarts its turn after a free-quota backoff before
/// giving up. The initial attempt is not a retry.
pub const MAX_RETRIES: u32 = 5;

/// Backoff before the `retry`-th restart (1-based): 1s, 2s, 4s, 8s, 16s,
/// capped at 30s so a long run still leaves room for the watchdog.
pub fn backoff(retry: u32) -> Duration {
    let shift = retry.saturating_sub(1).min(5);
    Duration::from_secs((1u64 << shift).min(30))
}

/// What the run lane does with a classified free-quota response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// Stop the run with this human-readable reason (shown in the desktop
    /// UI and the run events via `RunFailed.detail`).
    Stop(String),
    /// Back off and retry the turn; never fail the run on this notice.
    Retry { code: &'static str },
}

/// Classify a gateway answer on `lane` by its error `code`. `None` when the
/// lane is not `openhub`, the body carries no recognised code, or nothing
/// applies — the caller keeps today's behaviour.
pub fn classify(lane: &str, status: u64, body: &str) -> Option<Disposition> {
    if lane != OPENHUB_LANE {
        return None;
    }
    let parsed = parse_error_body(body);
    let code = parsed.code.as_deref()?;
    let disposition = match (status, code) {
        (_, CODE_EXHAUSTED) => Disposition::Stop(exhausted_reason(parsed.resets_at.as_deref())),
        (_, CODE_NEEDS_CREDIT) => Disposition::Stop("this model needs credit".to_string()),
        (_, CODE_WALLET_EMPTY | CODE_NO_CREDIT_ACCOUNT) => {
            Disposition::Stop("your wallet is empty; top up to continue".to_string())
        }
        (_, CODE_BUSY) => Disposition::Retry { code: CODE_BUSY },
        (_, CODE_UNAVAILABLE) => Disposition::Retry {
            code: CODE_UNAVAILABLE,
        },
        _ => return None,
    };
    Some(disposition)
}

/// The human sentence for a run that exhausted its backoff retries instead
/// of failing on the first busy/unavailable notice.
pub fn retry_exhausted_detail(code: &str, retries: u32) -> String {
    match code {
        CODE_BUSY => format!("free lane busy: still busy after {retries} backoff retries"),
        CODE_UNAVAILABLE => {
            format!("free quota unavailable: still unavailable after {retries} backoff retries")
        }
        _ => format!("{code}: still failing after {retries} backoff retries"),
    }
}

fn exhausted_reason(resets_at: Option<&str>) -> String {
    match resets_at.filter(|s| !s.trim().is_empty()) {
        Some(resets_at) => format!("daily free limit reached, resets at {}", resets_at.trim()),
        None => "daily free limit reached".to_string(),
    }
}

#[derive(Default)]
struct ErrorBody {
    code: Option<String>,
    resets_at: Option<String>,
}

/// The `code` (and, for the exhausted case, `resets_at`) out of an
/// OpenAI-style error body (`error.code`, or a top-level `code`). Bodies
/// that are not JSON with a string `code` yield nothing.
fn parse_error_body(body: &str) -> ErrorBody {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body.trim()) else {
        return ErrorBody::default();
    };
    let error = value.get("error");
    let str_field = |holder: Option<&serde_json::Value>, key: &str| {
        holder
            .and_then(|h| h.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    ErrorBody {
        code: str_field(error, "code").or_else(|| str_field(Some(&value), "code")),
        resets_at: str_field(error, "resets_at").or_else(|| str_field(Some(&value), "resets_at")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(code: &str) -> String {
        format!("{{\"error\":{{\"code\":\"{code}\",\"message\":\"gateway said so\"}}}}")
    }

    fn exhausted_body(resets_at: Option<&str>) -> String {
        match resets_at {
            Some(resets_at) => format!(
                "{{\"error\":{{\"code\":\"free_quota_exhausted\",\"limit\":2000000,\"plan\":\"free\",\"resets_at\":\"{resets_at}\"}}}}"
            ),
            None => body("free_quota_exhausted"),
        }
    }

    #[test]
    fn exhausted_stops_with_resets_at_kept_in_the_reason() {
        let disposition = classify(
            "openhub",
            402,
            &exhausted_body(Some("2026-11-09T00:00:00Z")),
        )
        .expect("exhausted classifies on the openhub lane");
        assert_eq!(
            disposition,
            Disposition::Stop(
                "daily free limit reached, resets at 2026-11-09T00:00:00Z".to_string()
            )
        );
        // Without a reset time the reason still names the limit.
        assert_eq!(
            classify("openhub", 402, &exhausted_body(None)),
            Some(Disposition::Stop("daily free limit reached".to_string()))
        );
    }

    #[test]
    fn needs_credit_and_wallet_empty_stop_with_their_reasons() {
        assert_eq!(
            classify("openhub", 402, &body("free_plan_paid_lane_blocked")),
            Some(Disposition::Stop("this model needs credit".to_string()))
        );
        for code in ["insufficient_balance", "no_credit_account"] {
            let disposition = classify("openhub", 402, &body(code))
                .unwrap_or_else(|| panic!("{code} must classify"));
            match disposition {
                Disposition::Stop(reason) => {
                    assert!(reason.contains("top up"), "{code}: {reason}")
                }
                Disposition::Retry { .. } => panic!("{code} must stop, not retry"),
            }
        }
    }

    #[test]
    fn busy_and_unavailable_retry_instead_of_stopping() {
        assert_eq!(
            classify("openhub", 429, &body("free_lane_busy")),
            Some(Disposition::Retry { code: CODE_BUSY })
        );
        assert_eq!(
            classify("openhub", 503, &body("free_quota_unavailable")),
            Some(Disposition::Retry {
                code: CODE_UNAVAILABLE
            })
        );
        // Retries are classified by code, so a status/code mix the gateway
        // might reword still retries rather than failing the run.
        assert_eq!(
            classify("openhub", 429, &body("free_quota_unavailable")),
            Some(Disposition::Retry {
                code: CODE_UNAVAILABLE
            })
        );
    }

    #[test]
    fn other_lanes_and_unrecognised_bodies_are_unaffected() {
        // The same bodies on any other lane classify as nothing, so those
        // lanes keep today's behaviour.
        for lane in ["", "byok", "other"] {
            assert_eq!(
                classify(lane, 402, &exhausted_body(Some("2026-11-09T00:00:00Z"))),
                None,
                "lane {lane:?} must not classify"
            );
            assert_eq!(classify(lane, 429, &body("free_lane_busy")), None);
        }
        // Prose, unknown codes and missing codes stay generic (no crash).
        assert_eq!(classify("openhub", 402, "Daily free quota used"), None);
        assert_eq!(classify("openhub", 402, &body("something_else")), None);
        assert_eq!(classify("openhub", 402, "{}"), None);
        assert_eq!(classify("openhub", 402, ""), None);
        // A top-level code (no error wrapper) still classifies.
        assert_eq!(
            classify("openhub", 429, r#"{"code": "free_lane_busy"}"#),
            Some(Disposition::Retry { code: CODE_BUSY })
        );
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(1), Duration::from_secs(1));
        assert_eq!(backoff(2), Duration::from_secs(2));
        assert_eq!(backoff(3), Duration::from_secs(4));
        assert_eq!(backoff(4), Duration::from_secs(8));
        assert_eq!(backoff(5), Duration::from_secs(16));
        assert_eq!(backoff(6), Duration::from_secs(30));
        assert_eq!(backoff(100), Duration::from_secs(30));
        assert_eq!(backoff(0), Duration::from_secs(1));
    }

    #[test]
    fn exhausted_retries_get_a_clear_message() {
        assert!(retry_exhausted_detail(CODE_BUSY, MAX_RETRIES).contains("busy"));
        assert!(retry_exhausted_detail(CODE_UNAVAILABLE, MAX_RETRIES).contains("unavailable"));
        assert!(retry_exhausted_detail("unknown", 2).contains("unknown"));
    }
}
