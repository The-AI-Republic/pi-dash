//! Dummy-data generation planning kernel (D-09, stage 5).
//!
//! Port of `apps/api/pi_dash/bgtasks/dummy_data_task.py:1-553` (the whole
//! file, one topological closure). This module owns the task's decisions —
//! row counts, sample sizes, sequence/sort progression, the five default
//! states, the fifteen-step orchestration order and the Celery payload
//! shape — over plain values, so `rust-api/fixtures/tasks_cleanup/`
//! (`dummy.json`, `columns.json`) replays here as unit tests without a
//! database. The jobs-layer `tasks_cleanup::dummy_data` module owns drawing,
//! fake strings, SQL execution and worker registration.
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * Insert order is [`CREATE_DUMMY_DATA_ORDER`], the call order of
//!   `create_dummy_data` (`:488-553`).
//! * `create_cycles` loops `while len(cycles) <= cycle_count` (`:155`),
//!   creating `cycle_count + 1` rows ([`cycle_row_count`]).
//! * `create_issue_parent` (`:378-388`) appends nothing to
//!   `bulk_sub_issues` and calls `bulk_update([])`: no parent link is ever
//!   persisted. Ported as [`parent_links_persisted`] returning 0 — the
//!   executor performs no write there.
//! * `random` is never seeded while `Faker.seed(0)` runs before the
//!   labels/cycles/modules/pages/issues sections (`:127,:147,:193,:223,`
//!   `:269`): counts and slices differ every run. The executor ports the
//!   unseeded draws as-is with an OS-seeded RNG.
//! * `text[:254]` / `name[:k]` slice code points, never bytes
//!   ([`truncate_code_points`]); the upper bound can fall below 2 for short
//!   fake names, in which case `randint` raises ([`identifier_bounds`]).
//! * Empty populations raise in Python (`randint(0, -1)`, oversized
//!   `sample`): the `*_size` helpers return `Err` there instead of a count.

use serde_json::Value;

/// Celery wire name, exactly as `.delay()` and the beat plane call it.
pub const TASK_NAME: &str = "pi_dash.bgtasks.dummy_data_task.create_dummy_data";

/// `create_dummy_data` call order (`dummy_data_task.py:488-553`).
pub const CREATE_DUMMY_DATA_ORDER: [&str; 15] = [
    "create_project",
    "create_project_members",
    "create_states",
    "create_labels",
    "create_cycles",
    "create_modules",
    "create_pages",
    "create_page_labels",
    "create_issues",
    "create_intake_issues",
    "create_issue_parent",
    "create_issue_assignees",
    "create_issue_labels",
    "create_cycle_issues",
    "create_module_issues",
];

/// The five states `create_states` inserts (`:82-123`).
#[derive(Debug, Clone, PartialEq)]
pub struct StateSeed {
    pub name: &'static str,
    pub color: &'static str,
    pub sequence: f64,
    pub group: &'static str,
    pub default: bool,
}

pub const DEFAULT_STATES: [StateSeed; 5] = [
    StateSeed {
        name: "Backlog",
        color: "#A3A3A3",
        sequence: 15000.0,
        group: "backlog",
        default: true,
    },
    StateSeed {
        name: "Todo",
        color: "#3A3A3A",
        sequence: 25000.0,
        group: "unstarted",
        default: false,
    },
    StateSeed {
        name: "In Progress",
        color: "#F59E0B",
        sequence: 35000.0,
        group: "started",
        default: false,
    },
    StateSeed {
        name: "Done",
        color: "#16A34A",
        sequence: 45000.0,
        group: "completed",
        default: false,
    },
    StateSeed {
        name: "Cancelled",
        color: "#EF4444",
        sequence: 55000.0,
        group: "cancelled",
        default: false,
    },
];

/// `IntakeIssueStatus` choices `create_intake_issues` draws from (`:368`).
pub const INTAKE_STATUSES: [i32; 5] = [-2, -1, 0, 1, 2];

/// `PRIORITY_CHOICES` keys `create_issues` draws from (`:312`).
pub const PRIORITIES: [&str; 5] = ["urgent", "high", "medium", "low", "none"];

/// Default intake name for `get_or_create` (`:360`).
pub const INTAKE_NAME: &str = "Intake";

/// Excluded state group for the issue creator pool (`:272-276`).
pub const TRIAGE_GROUP: &str = "triage";

/// `SourceType.IN_APP` stored on intake links (`:372`).
pub const SOURCE_IN_APP: &str = "IN_APP";

/// `IssueActivity` stamp per created issue (`:338-351`).
pub const ISSUE_ACTIVITY_VERB: &str = "created";
pub const ISSUE_ACTIVITY_COMMENT: &str = "created the issue";

/// `ProjectMember.role` for the creator and every bulk member (`:58,:71`).
pub const PROJECT_MEMBER_ROLE: i32 = 20;

/// `create_labels` always inserts 50 rows (`:134-142`).
pub const LABEL_ROWS: i64 = 50;

/// Sort-order seed and step (`:289-291,318`).
pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
pub const SORT_ORDER_STEP: f64 = 10000.0;

/// Upper draw for the per-row sort advance (`randint(0, 1000)`, `:318`) and
/// for the issue/module multi-links (`randint(0, 5)`, `:429,:477`).
pub const SORT_ADVANCE_DRAW_MAX: u32 = 1000;
pub const MULTI_LINK_DRAW_MAX: usize = 5;

/// `range(0, n)` inserts `max(n, 0)` rows; a negative count yields none.
fn range_rows(count: i64) -> i64 {
    count.max(0)
}

/// Module/page/issue/intake-issue row counts (`:196,:226,:297,:359`).
pub fn module_row_count(module_count: i64) -> i64 {
    range_rows(module_count)
}

/// Cycle rows (`:155`): the `<=` condition appends once more than asked.
pub fn cycle_row_count(cycle_count: i64) -> i64 {
    if cycle_count < 0 {
        0
    } else {
        cycle_count + 1
    }
}

pub fn page_row_count(pages_count: i64) -> i64 {
    range_rows(pages_count)
}

pub fn issue_row_count(issue_count: i64) -> i64 {
    range_rows(issue_count)
}

/// `int(pages_count / 2)` / `int(issue_count / 2)` sample sizes
/// (`:253-256,:397-400,:443-446`); negative counts raise in `sample`.
pub fn half_sample_size(count: i64) -> Result<usize, String> {
    if count < 0 {
        return Err(format!("sample size int({count} / 2) is negative"));
    }
    Ok((count as u64 / 2) as usize)
}

/// `int(issue_count / 4)` parent pool and `int(issue_count / 2)` sub-issue
/// slice (`:379-381`); negative counts raise before any write.
pub fn parent_plan(issue_count: i64) -> Result<(usize, usize), String> {
    if issue_count < 0 {
        return Err(format!("parent plan int({issue_count} / 4) is negative"));
    }
    Ok((
        (issue_count as u64 / 4) as usize,
        (issue_count as u64 / 2) as usize,
    ))
}

/// `create_issue_parent` persists no parent link (`:383-388`: the loop sets
/// `parent_id` in memory but never appends to `bulk_sub_issues`, then
/// `bulk_update([])` writes nothing).
pub fn parent_links_persisted() -> usize {
    0
}

/// `randint(0, len(population) - 1)` per-row draw count (`:260,:405`);
/// an empty population raises `ValueError`.
pub fn per_row_draw_max(population: usize) -> Result<usize, String> {
    if population == 0 {
        return Err("randint(0, -1) on an empty population".to_owned());
    }
    Ok(population - 1)
}

/// First `sequence_id`: `Max("sequence") + 1`, or 1 when none (`:283-285`).
pub fn next_sequence(max_sequence: Option<i64>) -> i64 {
    match max_sequence {
        None => 1,
        Some(largest) => largest + 1,
    }
}

/// First `sort_order`: max sort in a random state `+ 10000`, or 65535 when
/// no issue exists yet (`:288-291`).
pub fn initial_sort_order(max_sort: Option<f64>) -> f64 {
    match max_sort {
        None => DEFAULT_SORT_ORDER,
        Some(largest) => largest + SORT_ORDER_STEP,
    }
}

/// Per-row `sort_order += randint(0, 1000)` (`:318`).
pub fn advance_sort_order(sort_order: f64, draw: u32) -> f64 {
    sort_order + f64::from(draw)
}

/// `snoozed_till` is set exactly when the drawn status is snoozed (`:369`).
pub fn snoozed_for_status(status: i32) -> bool {
    status == 0
}

/// Python `text[:n]` counts code points; clamp to a char boundary so the
/// port of `name=text[:254]` (`:309`) never panics on UTF-8.
pub fn truncate_code_points(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => &text[..idx],
        None => text,
    }
}

/// `randint(2, 12 if len(name) - 1 >= 12 else len(name) - 1)` for the
/// project identifier (`:51`); short names push the upper bound below 2 and
/// `randint` raises.
pub fn identifier_bounds(name_chars: usize) -> Result<(u32, u32), String> {
    let upper = if name_chars.saturating_sub(1) >= 12 {
        12u32
    } else {
        name_chars.saturating_sub(1) as u32
    };
    if upper < 2 {
        return Err(format!("randint(2, {upper}) with empty range"));
    }
    Ok((2, upper))
}

/// The eight `create_dummy_data` arguments in signature order (`:489-497`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDummyDataArgs {
    pub slug: String,
    pub email: String,
    pub members: Vec<String>,
    pub issue_count: i64,
    pub cycle_count: i64,
    pub module_count: i64,
    pub pages_count: i64,
    pub intake_issue_count: i64,
}

impl CreateDummyDataArgs {
    /// Parse the positional Celery args array. Counts must be JSON integers
    /// (a bool or string fails, as `range()` would reject it in Python).
    pub fn from_job_args(args: &[Value]) -> Result<Self, String> {
        if args.len() != 8 {
            return Err(format!(
                "create_dummy_data takes 8 args, got {}",
                args.len()
            ));
        }
        let str_arg = |i: usize, name: &str| -> Result<String, String> {
            args[i]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("arg {name} must be a string"))
        };
        let int_arg = |i: usize, name: &str| -> Result<i64, String> {
            args[i]
                .as_i64()
                .ok_or_else(|| format!("arg {name} must be an integer"))
        };
        let members = args[2]
            .as_array()
            .ok_or_else(|| "arg members must be an array".to_owned())?;
        let mut member_emails = Vec::with_capacity(members.len());
        for member in members {
            member_emails.push(
                member
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "members entries must be emails".to_owned())?,
            );
        }
        Ok(Self {
            slug: str_arg(0, "slug")?,
            email: str_arg(1, "email")?,
            members: member_emails,
            issue_count: int_arg(3, "issue_count")?,
            cycle_count: int_arg(4, "cycle_count")?,
            module_count: int_arg(5, "module_count")?,
            pages_count: int_arg(6, "pages_count")?,
            intake_issue_count: int_arg(7, "intake_issue_count")?,
        })
    }

    /// The `.delay(...)` positional payload, signature order.
    pub fn wire_args(&self) -> Vec<Value> {
        vec![
            Value::String(self.slug.clone()),
            Value::String(self.email.clone()),
            Value::Array(
                self.members
                    .iter()
                    .map(|m| Value::String(m.clone()))
                    .collect(),
            ),
            Value::from(self.issue_count),
            Value::from(self.cycle_count),
            Value::from(self.module_count),
            Value::from(self.pages_count),
            Value::from(self.intake_issue_count),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn orchestration_order_matches_source() {
        assert_eq!(CREATE_DUMMY_DATA_ORDER.len(), 15);
        assert_eq!(CREATE_DUMMY_DATA_ORDER.first(), Some(&"create_project"));
        assert_eq!(
            CREATE_DUMMY_DATA_ORDER.last(),
            Some(&"create_module_issues")
        );
    }

    #[test]
    fn default_states_literal() {
        assert_eq!(DEFAULT_STATES.len(), 5);
        assert_eq!(DEFAULT_STATES[0].name, "Backlog");
        assert!(DEFAULT_STATES[0].default);
        assert!(DEFAULT_STATES[1..].iter().all(|s| !s.default));
        let groups: Vec<&str> = DEFAULT_STATES.iter().map(|s| s.group).collect();
        assert_eq!(
            groups,
            ["backlog", "unstarted", "started", "completed", "cancelled"]
        );
    }

    #[test]
    fn cycle_off_by_one_is_kept() {
        assert_eq!(cycle_row_count(0), 1);
        assert_eq!(cycle_row_count(3), 4);
        assert_eq!(cycle_row_count(-1), 0);
    }

    #[test]
    fn range_counts_clamp_negative() {
        assert_eq!(module_row_count(5), 5);
        assert_eq!(page_row_count(0), 0);
        assert_eq!(issue_row_count(-4), 0);
    }

    #[test]
    fn half_samples_truncate_like_int_div() {
        assert_eq!(half_sample_size(5), Ok(2));
        assert_eq!(half_sample_size(4), Ok(2));
        assert_eq!(half_sample_size(0), Ok(0));
        assert!(half_sample_size(-2).is_err());
    }

    #[test]
    fn parent_plan_sizes_and_noop() {
        assert_eq!(parent_plan(8), Ok((2, 4)));
        assert_eq!(parent_plan(5), Ok((1, 2)));
        assert!(parent_plan(-8).is_err());
        assert_eq!(parent_links_persisted(), 0);
    }

    #[test]
    fn per_row_draw_rejects_empty_population() {
        assert_eq!(per_row_draw_max(3), Ok(2));
        assert!(per_row_draw_max(0).is_err());
    }

    #[test]
    fn sequence_and_sort_progression() {
        assert_eq!(next_sequence(None), 1);
        assert_eq!(next_sequence(Some(41)), 42);
        assert_eq!(initial_sort_order(None), 65535.0);
        assert_eq!(initial_sort_order(Some(65535.0)), 75535.0);
        assert_eq!(advance_sort_order(75535.0, 250), 75785.0);
    }

    #[test]
    fn snooze_only_for_status_zero() {
        assert!(snoozed_for_status(0));
        assert!(!snoozed_for_status(-2));
        assert!(!snoozed_for_status(2));
    }

    #[test]
    fn truncation_counts_code_points_without_panic() {
        assert_eq!(truncate_code_points("abcdef", 4), "abcd");
        assert_eq!(truncate_code_points("abc", 9), "abc");
        assert_eq!(truncate_code_points("éééé", 2), "éé");
        assert_eq!(truncate_code_points("", 254), "");
    }

    #[test]
    fn identifier_bounds_match_source() {
        assert_eq!(identifier_bounds(20), Ok((2, 12)));
        assert_eq!(identifier_bounds(13), Ok((2, 12)));
        assert_eq!(identifier_bounds(8), Ok((2, 7)));
        assert!(identifier_bounds(2).is_err());
        assert!(identifier_bounds(0).is_err());
    }

    #[test]
    fn args_parse_and_wire_round_trip() {
        let wire = vec![
            json!("ws-slug"),
            json!("owner@example.com"),
            json!(["a@example.com", "b@example.com"]),
            json!(4),
            json!(2),
            json!(2),
            json!(4),
            json!(2),
        ];
        let parsed = CreateDummyDataArgs::from_job_args(&wire).expect("parse");
        assert_eq!(parsed.slug, "ws-slug");
        assert_eq!(parsed.members, ["a@example.com", "b@example.com"]);
        assert_eq!(parsed.issue_count, 4);
        assert_eq!(parsed.wire_args(), wire);
    }

    #[test]
    fn args_reject_wrong_arity_and_types() {
        assert!(CreateDummyDataArgs::from_job_args(&[]).is_err());
        let mut wire = vec![
            json!("s"),
            json!("e"),
            json!([]),
            json!(true),
            json!(0),
            json!(0),
            json!(0),
            json!(0),
        ];
        assert!(CreateDummyDataArgs::from_job_args(&wire).is_err());
        wire[3] = json!("4");
        assert!(CreateDummyDataArgs::from_job_args(&wire).is_err());
    }
}
