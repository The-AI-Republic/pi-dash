//! Canonical run-failure taxonomy (PDASHOSS01-183).
//!
//! Port of `apps/api/pi_dash/runner/failure.py`: the stored
//! [`RunFailureReason`] values, the per-reason [`FailurePolicy`] the ticker
//! consults ([`policy_for`]), the backstop limits
//! ([`repeated_failure_limit_for`]) and the write-path classifier
//! ([`classify`]). `consecutive_failure_streak` needs the database and lives
//! with the agent-run queries
//! (`pidash_db::runner_runs::agent_run::failure_streak_sql`).
//!
//! The string values are persisted and feed dashboards: a wire contract.
//! Renaming one is a breaking change — add new values instead.
//!
//! Translation notes:
//!
//! * The `regex` crate has no lookaround. Python's digit-boundary status
//!   fragment `(?<![0-9])(?:401|…)(?![0-9])` becomes
//!   `(?:^|[^0-9])(?:401|…)(?:[^0-9]|$)`; only `is_match` is ever asked,
//!   so consuming the neighbour character instead of asserting it gives
//!   the same answer.
//! * Python `re.IGNORECASE` on `str` also folds `İ` (U+0130) and `ı`
//!   (U+0131) onto `i`; the `regex` crate's simple case folding does not.
//!   The haystack is normalised for those two characters before the text
//!   rules run (both are word characters, so `\b` is unaffected, and the
//!   swap is one char for one char, so `.{0,40}` windows are unchanged).
//! * `str.strip()` / `str.lower()` follow the sibling diagnostics module:
//!   [`py_strip`] plus Rust's `to_lowercase` (same Unicode default
//!   mappings as CPython).
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-10-failure.golden.json`
//! (FX-RUN-10 `reasons`, `unknown_policy`, `classify`, `constants`).

use std::sync::OnceLock;

use regex::Regex;

/// `AGENT_ERROR_PREFIX` (`failure.py`): marks the agent-side group.
pub const AGENT_ERROR_PREFIX: &str = "agent_error.";

/// `FREE_RETRY_MAX_ATTEMPTS`: consecutive same-reason free retries allowed
/// before the backstop takes over.
pub const FREE_RETRY_MAX_ATTEMPTS: u32 = 3;

/// `FREE_RETRY_BACKOFF_SECONDS`: delay before a free retry fires.
pub const FREE_RETRY_BACKOFF_SECONDS: i64 = 120;

/// `REPEATED_FAILURE_LIMIT`: this many same-reason failures in a row stop
/// the clock regardless of policy.
pub const REPEATED_FAILURE_LIMIT: u32 = 3;

/// `RunFailureReason` (`failure.py`): the stored `AgentRun.failure_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunFailureReason {
    WorkspaceSetup,
    GitAuth,
    Network,
    RunnerOffline,
    DaemonRestart,
    Timeout,
    MaxTurns,
    AssignRejectedBusy,
    Internal,
    AgentProviderAuth,
    AgentProviderQuota,
    AgentProviderCapacity,
    AgentProviderServerError,
    AgentProviderNetwork,
    AgentContextOverflow,
    AgentModelNotFound,
    AgentMissingExecutable,
    AgentMissingConfig,
    AgentUnsupportedVersion,
    AgentEmptyOutput,
    AgentProcessFailure,
    AgentUnknown,
}

impl RunFailureReason {
    /// Every member in `TextChoices` declaration order.
    pub const ALL: [RunFailureReason; 22] = [
        RunFailureReason::WorkspaceSetup,
        RunFailureReason::GitAuth,
        RunFailureReason::Network,
        RunFailureReason::RunnerOffline,
        RunFailureReason::DaemonRestart,
        RunFailureReason::Timeout,
        RunFailureReason::MaxTurns,
        RunFailureReason::AssignRejectedBusy,
        RunFailureReason::Internal,
        RunFailureReason::AgentProviderAuth,
        RunFailureReason::AgentProviderQuota,
        RunFailureReason::AgentProviderCapacity,
        RunFailureReason::AgentProviderServerError,
        RunFailureReason::AgentProviderNetwork,
        RunFailureReason::AgentContextOverflow,
        RunFailureReason::AgentModelNotFound,
        RunFailureReason::AgentMissingExecutable,
        RunFailureReason::AgentMissingConfig,
        RunFailureReason::AgentUnsupportedVersion,
        RunFailureReason::AgentEmptyOutput,
        RunFailureReason::AgentProcessFailure,
        RunFailureReason::AgentUnknown,
    ];

    /// The stored wire value.
    pub fn value(&self) -> &'static str {
        match self {
            RunFailureReason::WorkspaceSetup => "workspace_setup",
            RunFailureReason::GitAuth => "git_auth",
            RunFailureReason::Network => "network",
            RunFailureReason::RunnerOffline => "runner_offline",
            RunFailureReason::DaemonRestart => "daemon_restart",
            RunFailureReason::Timeout => "timeout",
            RunFailureReason::MaxTurns => "max_turns",
            RunFailureReason::AssignRejectedBusy => "assign_rejected_busy",
            RunFailureReason::Internal => "internal",
            RunFailureReason::AgentProviderAuth => "agent_error.provider_auth_or_access",
            RunFailureReason::AgentProviderQuota => "agent_error.provider_quota_limit",
            RunFailureReason::AgentProviderCapacity => {
                "agent_error.provider_capacity_or_rate_limit"
            }
            RunFailureReason::AgentProviderServerError => "agent_error.provider_server_error",
            RunFailureReason::AgentProviderNetwork => "agent_error.provider_network",
            RunFailureReason::AgentContextOverflow => "agent_error.context_overflow",
            RunFailureReason::AgentModelNotFound => "agent_error.model_not_found_or_unavailable",
            RunFailureReason::AgentMissingExecutable => "agent_error.missing_executable",
            RunFailureReason::AgentMissingConfig => "agent_error.missing_config",
            RunFailureReason::AgentUnsupportedVersion => "agent_error.unsupported_version",
            RunFailureReason::AgentEmptyOutput => "agent_error.empty_or_unparseable_output",
            RunFailureReason::AgentProcessFailure => "agent_error.process_failure",
            RunFailureReason::AgentUnknown => "agent_error.unknown",
        }
    }

    /// The `TextChoices` label (human-facing, used in the failure comment).
    pub fn label(&self) -> &'static str {
        match self {
            RunFailureReason::WorkspaceSetup => "Workspace setup failed",
            RunFailureReason::GitAuth => "Git authentication failed",
            RunFailureReason::Network => "Runner network failure",
            RunFailureReason::RunnerOffline => "Runner offline",
            RunFailureReason::DaemonRestart => "Runner daemon restarted",
            RunFailureReason::Timeout => "Run timed out",
            RunFailureReason::MaxTurns => "Agent hit its turn budget",
            RunFailureReason::AssignRejectedBusy => "Runner busy, assignment rejected",
            RunFailureReason::Internal => "Internal error",
            RunFailureReason::AgentProviderAuth => "Provider authentication or access failed",
            RunFailureReason::AgentProviderQuota => "Provider quota or credit limit",
            RunFailureReason::AgentProviderCapacity => "Provider capacity or rate limit",
            RunFailureReason::AgentProviderServerError => "Provider server error",
            RunFailureReason::AgentProviderNetwork => "Provider network failure",
            RunFailureReason::AgentContextOverflow => "Context window overflow",
            RunFailureReason::AgentModelNotFound => "Model not found or unavailable",
            RunFailureReason::AgentMissingExecutable => "Agent executable missing",
            RunFailureReason::AgentMissingConfig => "Agent configuration missing",
            RunFailureReason::AgentUnsupportedVersion => "Unsupported agent version",
            RunFailureReason::AgentEmptyOutput => "Empty or unparseable agent output",
            RunFailureReason::AgentProcessFailure => "Agent process failed",
            RunFailureReason::AgentUnknown => "Unclassified agent error",
        }
    }

    /// `RunFailureReason(value)` for a known value, `None` otherwise
    /// (`reason in RunFailureReason.values`). Exact match — no stripping.
    pub fn from_value(value: &str) -> Option<RunFailureReason> {
        RunFailureReason::ALL
            .into_iter()
            .find(|reason| reason.value() == value)
    }
}

impl std::fmt::Display for RunFailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// `is_agent_side` (`failure.py`): the `agent_error.` prefix test.
pub fn is_agent_side(reason: &str) -> bool {
    reason.starts_with(AGENT_ERROR_PREFIX)
}

/// `FailurePolicy` (`failure.py`): what the ticker does about a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailurePolicy {
    /// Transient infrastructure: re-run on a short backoff without
    /// spending a tick.
    RetryFree,
    /// Maybe-transient: keep ticking, spend a tick.
    RetryNormal,
    /// A retry fails identically until a human acts: stop the clock.
    NeedsHuman,
    /// The next run must start a fresh session; ticks on the normal clock.
    FreshSession,
}

impl FailurePolicy {
    /// The wire value.
    pub fn value(&self) -> &'static str {
        match self {
            FailurePolicy::RetryFree => "retry_free",
            FailurePolicy::RetryNormal => "retry_normal",
            FailurePolicy::NeedsHuman => "needs_human",
            FailurePolicy::FreshSession => "fresh_session",
        }
    }
}

/// `policy_for` (`failure.py`): the ticker policy for a stored reason.
/// Unknown values (including `""`) fail safe onto the normal clock — never
/// into the free-retry bucket. Exact match, like the Python dict lookup.
pub fn policy_for(reason: &str) -> FailurePolicy {
    match RunFailureReason::from_value(reason) {
        Some(
            RunFailureReason::Network
            | RunFailureReason::RunnerOffline
            | RunFailureReason::DaemonRestart
            | RunFailureReason::AssignRejectedBusy
            | RunFailureReason::AgentProviderNetwork,
        ) => FailurePolicy::RetryFree,
        Some(
            RunFailureReason::WorkspaceSetup
            | RunFailureReason::GitAuth
            | RunFailureReason::AgentProviderAuth
            | RunFailureReason::AgentProviderQuota
            | RunFailureReason::AgentModelNotFound
            | RunFailureReason::AgentMissingExecutable
            | RunFailureReason::AgentMissingConfig
            | RunFailureReason::AgentUnsupportedVersion,
        ) => FailurePolicy::NeedsHuman,
        Some(RunFailureReason::AgentContextOverflow) => FailurePolicy::FreshSession,
        _ => FailurePolicy::RetryNormal,
    }
}

/// `repeated_failure_limit_for` (`failure.py`): the consecutive
/// same-reason streak at which the backstop stops the clock. Free-retry
/// reasons get their retry allowance first.
pub fn repeated_failure_limit_for(reason: &str) -> u32 {
    if policy_for(reason) == FailurePolicy::RetryFree {
        REPEATED_FAILURE_LIMIT.max(FREE_RETRY_MAX_ATTEMPTS + 1)
    } else {
        REPEATED_FAILURE_LIMIT
    }
}

/// `consecutive_failure_streak`'s scan window (`failure.py`):
/// `REPEATED_FAILURE_LIMIT + FREE_RETRY_MAX_ATTEMPTS + 2`.
pub const FAILURE_STREAK_WINDOW: u32 = REPEATED_FAILURE_LIMIT + FREE_RETRY_MAX_ATTEMPTS + 2;

/// The streak count over the newest-first `(status, failure_reason)` rows
/// `consecutive_failure_streak` reads: stops at the first row that is not
/// `FAILED`-with-`reason` (a `NULL`/empty reason reads as `""`).
pub fn failure_streak_from_rows<'a, I>(rows: I, reason: &str) -> u32
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut streak = 0;
    for (status, stored) in rows {
        if status != "failed" || stored != reason {
            break;
        }
        streak += 1;
    }
    streak
}

/// `_RUNNER_REASON_MAP` (`failure.py`): runner-protocol reasons that map
/// 1:1 onto a platform reason.
fn runner_reason_map(key: &str) -> Option<RunFailureReason> {
    Some(match key {
        "workspace_setup" => RunFailureReason::WorkspaceSetup,
        "git_auth" => RunFailureReason::GitAuth,
        "network" => RunFailureReason::Network,
        "max_turns" => RunFailureReason::MaxTurns,
        "timeout" => RunFailureReason::Timeout,
        "daemon_restart" => RunFailureReason::DaemonRestart,
        "assign_rejected_busy" => RunFailureReason::AssignRejectedBusy,
        "resume_unavailable" => RunFailureReason::Internal,
        "cancelled" => RunFailureReason::Internal,
        _ => return None,
    })
}

/// `_ERROR_CODE_MAP` (`failure.py`): cloud-side `error_code` hints,
/// consulted only when no runner reason is available.
fn error_code_map(key: &str) -> Option<RunFailureReason> {
    Some(match key {
        "run_timeout" | "dispatch_timeout" => RunFailureReason::Timeout,
        "heartbeat_reaped" | "desktop_not_connected" => RunFailureReason::RunnerOffline,
        "llm_config_missing" | "gateway_scopes_missing" | "byok_not_supported_on_desktop" => {
            RunFailureReason::AgentMissingConfig
        }
        "cloud_agent_disabled" | "managed_runner_disabled" | "actor_no_longer_authorized" => {
            RunFailureReason::Internal
        }
        "prompt_too_large" => RunFailureReason::AgentContextOverflow,
        "final_result_too_large" => RunFailureReason::AgentEmptyOutput,
        _ => return None,
    })
}

/// `_INFRA_TEXT_PREFIXES` (`failure.py`), in tuple order: cloud-written
/// detail prefixes (matched on the lowercased detail).
const INFRA_TEXT_PREFIXES: [(&str, RunFailureReason); 6] = [
    ("daemon shutdown requested", RunFailureReason::DaemonRestart),
    ("agent stalled: no events for >", RunFailureReason::Timeout),
    ("no agent frames for", RunFailureReason::Timeout),
    ("reaped by heartbeat", RunFailureReason::RunnerOffline),
    ("cloud agent worker was lost", RunFailureReason::Timeout),
    (
        "pi dash agent never came online",
        RunFailureReason::RunnerOffline,
    ),
];

/// `_status(*codes)` without lookaround (see module notes).
fn status(codes: &[u16]) -> String {
    let joined = codes
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join("|");
    format!("(?:^|[^0-9])(?:{joined})(?:[^0-9]|$)")
}

/// `_AGENT_TEXT_RULES` (`failure.py`), in order — first match wins. Order
/// is load-bearing (context before quota, capacity before auth, model
/// access before auth, provider rules before the generic process rules).
fn agent_text_rules() -> &'static [(RunFailureReason, Regex)] {
    static RULES: OnceLock<Vec<(RunFailureReason, Regex)>> = OnceLock::new();
    RULES.get_or_init(|| {
        let rules: Vec<(RunFailureReason, String)> = vec![
            (
                RunFailureReason::AgentContextOverflow,
                concat!(
                    r"context[ _]window|context[ _]length|context_length_exceeded",
                    r"|prompt is too long|maximum context|exceeds? the (?:model'?s )?context",
                    r"|conversation (?:is )?too long|input length and `?max_tokens`? exceed",
                )
                .to_string(),
            ),
            (
                RunFailureReason::AgentProviderCapacity,
                status(&[429, 529])
                    + concat!(
                        r"|rate[ _-]?limit|too many requests|overloaded_error|\boverloaded\b",
                        r"|concurrent connections|capacity constraint|server is busy",
                        r"|number of request tokens has exceeded",
                    ),
            ),
            (
                RunFailureReason::AgentProviderQuota,
                status(&[402])
                    + concat!(
                        r"|insufficient (?:credit|quota|funds)|credit balance|out of (?:usage )?credits",
                        r"|quota exceeded|usage limit|billing|payment required",
                        r"|\b(?:5-hour|weekly|monthly) limit\b|hard limit reached",
                    ),
            ),
            (
                RunFailureReason::AgentModelNotFound,
                concat!(
                    r"selected model|model .{0,40}(?:not (?:found|exist|available)|may not exist)",
                    r"|unknown model|model_not_found|no such model",
                    r"|model is not supported|may not have access to the model",
                )
                .to_string(),
            ),
            (
                RunFailureReason::AgentProviderAuth,
                status(&[401, 403, 407])
                    + concat!(
                        r"|authentication_failed|failed to authenticate|invalid authentication",
                        r"|\bunauthorized\b|\bforbidden\b|invalid (?:api|access) key",
                        r"|api key.{0,30}(?:invalid|expired|revoked|disabled)",
                        r"|token (?:has )?expired|refresh(?:ing)? (?:the )?(?:oauth )?token",
                        r"|re-?authenticate|please run /login|not logged in|login required",
                        r"|invalid credentials|oauth",
                    ),
            ),
            (
                RunFailureReason::AgentMissingExecutable,
                concat!(
                    r"command not found|not recognized as an internal or external command",
                    r"|(?:executable|program|binary) (?:file )?not found",
                    r"|no such file or directory \(os error 2\)",
                    r"|failed to (?:spawn|launch|exec)",
                )
                .to_string(),
            ),
            (
                RunFailureReason::AgentMissingConfig,
                concat!(
                    r"missing (?:api key|configuration|config)",
                    r"|api key (?:is )?not (?:set|configured|found)|no api key",
                    r"|config(?:uration)? (?:file )?(?:not found|missing)",
                    r"|environment variable .{0,40}not set",
                )
                .to_string(),
            ),
            (
                RunFailureReason::AgentUnsupportedVersion,
                concat!(
                    r"unsupported (?:\w+ )?version|version mismatch",
                    r"|requires (?:at least )?version|please (?:upgrade|update) (?:to|your)",
                )
                .to_string(),
            ),
            (
                RunFailureReason::AgentProviderServerError,
                status(&[500, 502, 503, 504])
                    + concat!(
                        r"|internal server error|bad gateway|service unavailable",
                        r"|gateway time-?out|\bapi_error\b|\bserver_error\b|upstream error",
                    ),
            ),
            (
                RunFailureReason::AgentProviderNetwork,
                concat!(
                    r"connection (?:refused|reset|closed|aborted|error|timed out)",
                    r"|econnreset|econnrefused|etimedout|enotfound|eai_again",
                    r"|dns (?:error|failure|resolution)|could not resolve host",
                    r"|network (?:error|is unreachable)|no route to host",
                    r"|socket hang ?up|fetch failed|tls handshake|certificate (?:verify|error)",
                )
                .to_string(),
            ),
            (
                RunFailureReason::AgentEmptyOutput,
                concat!(
                    r"done-signal parse error|empty (?:output|response|result)",
                    r"|produced no (?:output|result)|no output received",
                    r"|failed to parse|unexpected end of (?:json|input)",
                )
                .to_string(),
            ),
            (
                RunFailureReason::AgentProcessFailure,
                concat!(
                    r"exit(?:ed)? (?:with )?(?:status|code)|non-?zero exit",
                    r"|\bsig(?:kill|segv|term|abrt|bus)\b|terminated by signal",
                    r"|panicked|core dumped|\bcrashed\b|process (?:died|exited|killed)",
                )
                .to_string(),
            ),
        ];
        rules
            .into_iter()
            .map(|(reason, pattern)| {
                let regex = Regex::new(&format!("(?i){pattern}"))
                    .expect("agent text rule compiles");
                (reason, regex)
            })
            .collect()
    })
}

/// Python `str.strip()` (the diagnostics-module definition).
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Fold the two characters Python `re.IGNORECASE` maps onto `i` that the
/// `regex` crate does not (see module notes).
fn fold_for_rules(detail: &str) -> std::borrow::Cow<'_, str> {
    if detail.contains(['\u{130}', '\u{131}']) {
        std::borrow::Cow::Owned(detail.replace(['\u{130}', '\u{131}'], "i"))
    } else {
        std::borrow::Cow::Borrowed(detail)
    }
}

/// `_classify_agent_text` (`failure.py`).
fn classify_agent_text(detail: &str) -> RunFailureReason {
    let haystack = fold_for_rules(detail);
    agent_text_rules()
        .iter()
        .find(|(_, pattern)| pattern.is_match(&haystack))
        .map(|(reason, _)| *reason)
        .unwrap_or(RunFailureReason::AgentUnknown)
}

/// `classify(raw_error, *, runner_reason="", error_code="")`
/// (`failure.py`): the canonical reason for a failed run. Platform-side
/// reasons win by construction; the agent-text rules run only when the
/// runner says the agent crashed (or reports `internal`), or when nothing
/// structured is known.
pub fn classify(raw_error: &str, runner_reason: &str, error_code: &str) -> RunFailureReason {
    let reason_key = py_strip(runner_reason).to_lowercase();
    if let Some(reason) = runner_reason_map(&reason_key) {
        return reason;
    }
    let detail = py_strip(raw_error);
    if reason_key == "agent_crash" || reason_key == "codex_crash" {
        return classify_agent_text(detail);
    }
    if reason_key == "internal" {
        // Deployed runners' Codex bridge reports agent-side turn / API
        // errors as `internal`; only a detail that says nothing agent-ish
        // stays platform-internal.
        return match classify_agent_text(detail) {
            RunFailureReason::AgentUnknown => RunFailureReason::Internal,
            other => other,
        };
    }
    let code_key = py_strip(error_code).to_lowercase();
    if let Some(reason) = error_code_map(&code_key) {
        return reason;
    }
    let lowered = detail.to_lowercase();
    for (prefix, reason) in INFRA_TEXT_PREFIXES {
        if lowered.starts_with(prefix) {
            return reason;
        }
    }
    classify_agent_text(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-10-failure.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("FX-RUN-10 parses")
    }

    fn s<'v>(value: &'v Value, key: &str) -> &'v str {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{key} is a string"))
    }

    #[test]
    fn constants_match_fixture() {
        let fx = fixture();
        let c = fx.get("constants").expect("constants");
        assert_eq!(c["FREE_RETRY_MAX_ATTEMPTS"], FREE_RETRY_MAX_ATTEMPTS);
        assert_eq!(c["FREE_RETRY_BACKOFF_SECONDS"], FREE_RETRY_BACKOFF_SECONDS);
        assert_eq!(c["REPEATED_FAILURE_LIMIT"], REPEATED_FAILURE_LIMIT);
        assert_eq!(c["AGENT_ERROR_PREFIX"], AGENT_ERROR_PREFIX);
        assert_eq!(FAILURE_STREAK_WINDOW, 8);
    }

    #[test]
    fn reasons_labels_and_policies_match_fixture_in_order() {
        let fx = fixture();
        let rows = fx["reasons"].as_array().expect("reasons");
        assert_eq!(rows.len(), RunFailureReason::ALL.len());
        for (row, reason) in rows.iter().zip(RunFailureReason::ALL) {
            assert_eq!(s(row, "value"), reason.value());
            assert_eq!(s(row, "label"), reason.label(), "{reason}");
            assert_eq!(
                s(row, "policy"),
                policy_for(reason.value()).value(),
                "{reason}"
            );
            assert_eq!(row["agent_side"], is_agent_side(reason.value()), "{reason}");
            assert_eq!(
                row["repeated_failure_limit"],
                repeated_failure_limit_for(reason.value()),
                "{reason}"
            );
            assert_eq!(RunFailureReason::from_value(reason.value()), Some(reason));
        }
        for (value, policy) in fx["unknown_policy"].as_object().expect("unknown policy") {
            assert_eq!(
                policy.as_str(),
                Some(policy_for(value).value()),
                "policy_for({value:?})"
            );
            assert_eq!(RunFailureReason::from_value(value), None);
        }
    }

    #[test]
    fn classify_replays_every_fixture_vector() {
        let fx = fixture();
        let rows = fx["classify"].as_array().expect("classify rows");
        assert_eq!(rows.len(), 944, "golden covers the tables");
        for row in rows {
            let raw = s(row, "raw");
            let runner_reason = s(row, "runner_reason");
            let error_code = s(row, "error_code");
            assert_eq!(
                classify(raw, runner_reason, error_code).value(),
                s(row, "out"),
                "classify({raw:?}, runner_reason={runner_reason:?}, error_code={error_code:?})"
            );
        }
    }

    #[test]
    fn digit_boundaries_never_select_a_provider_bucket() {
        // The issue's named false positives (also in the golden).
        assert_eq!(
            classify("agent exited with status 4030", "agent_crash", ""),
            RunFailureReason::AgentProcessFailure
        );
        assert_eq!(
            classify("agent crashed after 15290ms", "agent_crash", ""),
            RunFailureReason::AgentProcessFailure
        );
        assert_eq!(
            classify("consumed 402913 tokens then crashed", "agent_crash", ""),
            RunFailureReason::AgentProcessFailure
        );
        assert_eq!(
            classify("401", "agent_crash", ""),
            RunFailureReason::AgentProviderAuth
        );
    }

    #[test]
    fn dotless_and_dotted_i_fold_like_python() {
        // `re.IGNORECASE` folds U+0130/U+0131 onto `i` (verified in CPython).
        assert_eq!(
            classify("ınvalıd credentials", "agent_crash", ""),
            RunFailureReason::AgentProviderAuth
        );
        assert_eq!(
            classify("İnvalid credentials", "agent_crash", ""),
            RunFailureReason::AgentProviderAuth
        );
    }

    #[test]
    fn failure_streak_counts_until_the_first_break() {
        let reason = "agent_error.provider_network";
        let rows = [
            ("failed", reason),
            ("failed", reason),
            ("completed", ""),
            ("failed", reason),
        ];
        assert_eq!(failure_streak_from_rows(rows, reason), 2);
        assert_eq!(failure_streak_from_rows([("failed", "timeout")], reason), 0);
        assert_eq!(failure_streak_from_rows(std::iter::empty(), reason), 0);
        assert_eq!(failure_streak_from_rows([("queued", reason)], reason), 0);
    }
}
