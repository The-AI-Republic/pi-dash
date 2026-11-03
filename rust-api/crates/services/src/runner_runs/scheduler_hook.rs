#![forbid(unsafe_code)]

//! Scheduler-binding terminate hook (D-15, stage 5).
//!
//! Pure port of `apps/api/pi_dash/runner/services/scheduler_hook.py:1-42`
//! (`update_scheduler_binding_on_terminate`): `COMPLETED` clears a
//! non-empty `last_error`; `FAILED`/`CANCELLED`/`REFUSED` set it
//! (refusals as `refused (<category>)[: <error>]`, other failures as
//! the error or the status value), truncated to
//! `LAST_ERROR_MAX_LEN` *chars*; anything else (including `BLOCKED`
//! and a missing binding) is a no-op that leaves `updated_at`
//! untouched. `last_error` comparisons are exact: an unchanged value
//! means no write.
//!
//! Fixture: FX-RUN-05 (`scheduler_hook` section).

use pidash_db::tasks_ticker::models::LAST_ERROR_MAX_LEN;
use pidash_types::runner_runs::AgentRunStatus;
use uuid::Uuid;

use super::truncate_chars;

/// The bound run + binding facts the hook reads (from the locked
/// effects fetch's `select_related("scheduler_binding")` cache — the
/// hook itself issues no `SELECT`).
#[derive(Debug, Clone, PartialEq)]
pub struct BindingFacts {
    pub id: Uuid,
    pub last_error: String,
}

/// Outcome of [`plan_scheduler_hook`]: no write, a clear, or a set.
/// Both writes also bump `updated_at` to `now()`; the no-op writes
/// nothing at all.
#[derive(Debug, Clone, PartialEq)]
pub enum SchedulerHookPlan {
    Noop,
    ClearError { binding_id: Uuid },
    SetError { binding_id: Uuid, message: String },
}

/// Plan `update_scheduler_binding_on_terminate`. `refusal_category`
/// and `error` are the locked run row's values (`""` when unset).
/// Char-based truncation (`raw[:1000]` counts code points, so a
/// byte-based cut would keep fewer characters on non-ASCII input).
pub fn plan_scheduler_hook(
    binding: Option<&BindingFacts>,
    status: AgentRunStatus,
    refusal_category: &str,
    error: &str,
) -> SchedulerHookPlan {
    let Some(binding) = binding else {
        return SchedulerHookPlan::Noop;
    };
    if status == AgentRunStatus::Completed {
        if binding.last_error.is_empty() {
            return SchedulerHookPlan::Noop;
        }
        return SchedulerHookPlan::ClearError {
            binding_id: binding.id,
        };
    }
    if !matches!(
        status,
        AgentRunStatus::Failed | AgentRunStatus::Cancelled | AgentRunStatus::Refused
    ) {
        return SchedulerHookPlan::Noop;
    }
    let raw = if status == AgentRunStatus::Refused {
        let category = if refusal_category.is_empty() {
            "unknown"
        } else {
            refusal_category
        };
        let mut raw = format!("refused ({category})");
        if !error.is_empty() {
            raw.push_str(": ");
            raw.push_str(error);
        }
        raw
    } else if error.is_empty() {
        // `run.error or run.status`: the status member is a `str`
        // enum, so the fallback slices as its value.
        status.value().to_owned()
    } else {
        error.to_owned()
    };
    let message = truncate_chars(&raw, LAST_ERROR_MAX_LEN);
    if binding.last_error == message {
        return SchedulerHookPlan::Noop;
    }
    SchedulerHookPlan::SetError {
        binding_id: binding.id,
        message,
    }
}

/// Binding `UPDATE` (`scheduler_hook.py:29,42`):
/// `save(update_fields=["last_error", "updated_at"])`. Params: `$1`
/// the message (possibly `""`), `$2` now, `$3` binding id. Zero rows
/// raise `DatabaseError`.
pub fn scheduler_binding_update_sql() -> &'static str {
    "UPDATE \"scheduler_bindings\" SET \"last_error\" = $1, \"updated_at\" = $2 \
     WHERE \"scheduler_bindings\".\"id\" = $3"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-05-lifecycle.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn scheduler_cases(fx: &Value) -> Vec<Value> {
        fx.get("scheduler_hook")
            .and_then(Value::as_array)
            .expect("scheduler_hook section")
            .clone()
    }

    fn case<'a>(cases: &'a [Value], name: &str) -> &'a Value {
        cases
            .iter()
            .find(|c| c.get("case").and_then(Value::as_str) == Some(name))
            .unwrap_or_else(|| panic!("case {name}"))
    }

    fn binding(last_error: &str) -> BindingFacts {
        BindingFacts {
            id: Uuid::nil(),
            last_error: last_error.to_owned(),
        }
    }

    #[test]
    fn last_error_max_len_is_shared_const() {
        let fx = fixture();
        assert_eq!(
            fx.get("LAST_ERROR_MAX_LEN").and_then(Value::as_u64),
            Some(LAST_ERROR_MAX_LEN as u64)
        );
        assert_eq!(LAST_ERROR_MAX_LEN, 1000);
    }

    #[test]
    fn completed_clears_nonempty_error() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "completed-clears");
        let facts = binding("kaboom");
        let plan = plan_scheduler_hook(Some(&facts), AgentRunStatus::Completed, "", "");
        assert_eq!(
            plan,
            SchedulerHookPlan::ClearError {
                binding_id: Uuid::nil()
            }
        );
        assert_eq!(
            gold.get("last_error_after").and_then(Value::as_str),
            Some("")
        );
        assert_eq!(
            gold.get("updated_at_changed").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn completed_empty_error_is_noop() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "completed-already-empty");
        let facts = binding("");
        assert_eq!(
            plan_scheduler_hook(Some(&facts), AgentRunStatus::Completed, "", ""),
            SchedulerHookPlan::Noop
        );
        assert_eq!(
            gold.get("updated_at_changed").and_then(Value::as_bool),
            Some(false),
            "no save means updated_at untouched",
        );
    }

    #[test]
    fn failed_sets_error() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "failed-sets");
        let facts = binding("");
        assert_eq!(
            plan_scheduler_hook(Some(&facts), AgentRunStatus::Failed, "", "kaboom"),
            SchedulerHookPlan::SetError {
                binding_id: Uuid::nil(),
                message: "kaboom".to_owned(),
            }
        );
        assert_eq!(
            gold.get("last_error_after").and_then(Value::as_str),
            Some("kaboom")
        );
    }

    #[test]
    fn failed_empty_error_falls_back_to_status() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "failed-empty-error-uses-status");
        let facts = binding("stale");
        assert_eq!(
            plan_scheduler_hook(Some(&facts), AgentRunStatus::Failed, "", ""),
            SchedulerHookPlan::SetError {
                binding_id: Uuid::nil(),
                message: "failed".to_owned(),
            }
        );
        assert_eq!(
            gold.get("last_error_after").and_then(Value::as_str),
            Some("failed")
        );
    }

    #[test]
    fn cancelled_sets_error() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "cancelled-sets");
        let facts = binding("");
        assert_eq!(
            plan_scheduler_hook(Some(&facts), AgentRunStatus::Cancelled, "", "bye"),
            SchedulerHookPlan::SetError {
                binding_id: Uuid::nil(),
                message: "bye".to_owned(),
            }
        );
        assert_eq!(
            gold.get("last_error_after").and_then(Value::as_str),
            Some("bye")
        );
    }

    #[test]
    fn refused_renders_category_and_error() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "refused-with-category+error");
        let facts = binding("");
        assert_eq!(
            plan_scheduler_hook(Some(&facts), AgentRunStatus::Refused, "cyber", "declined"),
            SchedulerHookPlan::SetError {
                binding_id: Uuid::nil(),
                message: "refused (cyber): declined".to_owned(),
            }
        );
        assert_eq!(
            gold.get("last_error_after").and_then(Value::as_str),
            Some("refused (cyber): declined")
        );
    }

    #[test]
    fn refused_empty_category_falls_back_to_unknown() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "refused-empty-category");
        let facts = binding("");
        assert_eq!(
            plan_scheduler_hook(Some(&facts), AgentRunStatus::Refused, "", ""),
            SchedulerHookPlan::SetError {
                binding_id: Uuid::nil(),
                message: "refused (unknown)".to_owned(),
            }
        );
        assert_eq!(
            gold.get("last_error_after").and_then(Value::as_str),
            Some("refused (unknown)")
        );
    }

    #[test]
    fn failed_truncates_to_1000_chars() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "failed-truncates");
        let len_case = case(&cases, "truncated_len");
        let long = "E".repeat(1200);
        let facts = binding("");
        let plan = plan_scheduler_hook(Some(&facts), AgentRunStatus::Failed, "", &long);
        let SchedulerHookPlan::SetError { message, .. } = plan else {
            panic!("expected SetError, got {plan:?}");
        };
        assert_eq!(message.len(), 1000);
        assert_eq!(
            gold.get("last_error_after").and_then(Value::as_str),
            Some(message.as_str())
        );
        assert_eq!(len_case.get("len").and_then(Value::as_u64), Some(1000));
        // Chars, not bytes: 1200 em-dashes keep 1000 of them (3000 bytes).
        let wide = "—".repeat(1200);
        let plan = plan_scheduler_hook(Some(&facts), AgentRunStatus::Failed, "", &wide);
        let SchedulerHookPlan::SetError { message, .. } = plan else {
            panic!("expected SetError, got {plan:?}");
        };
        assert_eq!(message.chars().count(), 1000);
        assert_eq!(message, "—".repeat(1000));
    }

    #[test]
    fn unchanged_message_is_noop() {
        let facts = binding("kaboom");
        assert_eq!(
            plan_scheduler_hook(Some(&facts), AgentRunStatus::Failed, "", "kaboom"),
            SchedulerHookPlan::Noop
        );
    }

    #[test]
    fn no_binding_is_noop() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "no-binding");
        assert_eq!(
            plan_scheduler_hook(None, AgentRunStatus::Failed, "", "kaboom"),
            SchedulerHookPlan::Noop
        );
        assert_eq!(
            gold.get("outcome").and_then(Value::as_str),
            Some("returned-None-no-row-touched")
        );
    }

    #[test]
    fn non_terminal_and_blocked_are_noop() {
        let fx = fixture();
        let cases = scheduler_cases(&fx);
        let gold = case(&cases, "non-terminal");
        let facts = binding("keep");
        for status in [AgentRunStatus::Running, AgentRunStatus::Blocked] {
            assert_eq!(
                plan_scheduler_hook(Some(&facts), status, "", "x"),
                SchedulerHookPlan::Noop,
                "status {status}",
            );
        }
        assert_eq!(
            gold.get("last_error_after").and_then(Value::as_str),
            Some("keep")
        );
    }

    #[test]
    fn binding_update_sql_shape() {
        assert_eq!(
            scheduler_binding_update_sql(),
            "UPDATE \"scheduler_bindings\" SET \"last_error\" = $1, \"updated_at\" = $2 \
             WHERE \"scheduler_bindings\".\"id\" = $3"
        );
    }
}
