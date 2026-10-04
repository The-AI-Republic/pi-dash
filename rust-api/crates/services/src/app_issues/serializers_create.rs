//! IssueCreateSerializer port: validate, create/update writes, representation.
//!
//! Port of `IssueCreateSerializer`
//! (`apps/api/pi_dash/app/serializers/issue.py:136-520`):
//!
//! * `to_representation` (`:185-191`) → [`issue_create_to_representation`]:
//!   the 41-key wire shape ([`ISSUE_CREATE_FIELDS`], order verified live on
//!   the pinned Django 4.2.30 + DRF 3.15.2) with the `assignee_ids` /
//!   `label_ids` `initial_data` echo appended last. The create view
//!   re-queries instead of returning this (FX-ISS-14 owns that shape), but
//!   draft-to-issue returns `serializer.data` verbatim
//!   (`app/views/workspace/draft.py:307`), so the echo is wire-observable.
//! * `validate_complexity_score` (`:193-204`) →
//!   [`validate_complexity_score`].
//! * `validate` (`:206-385`) → [`issue_create_validate`], a pure kernel over
//!   injected probes: the 11 ordered steps, first failure wins, returning
//!   the mutated attrs (sanitized HTML, filtered id lists) on success.
//! * `create` (`:387-462`) / `update` (`:464-518`) → the `*_SQL` consts,
//!   [`m2m_insert_sql`], [`m2m_batches`], and
//!   [`create_default_assignee_fallback`]. Services executes no SQL; the
//!   handlers layer (PIDASHCONV-651) runs these specs in the documented
//!   order with the documented swallow rules.
//!
//! Reuse (merged, called not copied): [`super::serializers_engage`]'s
//! [`issue_is_actively_synced`](super::serializers_engage::issue_is_actively_synced)
//! for the sync lock; [`AgentExecutorKind`], [`ManagedRunnerReason`],
//! [`cloud_agent_is_configured`], [`managed_runner_availability`] for the
//! executor branch; [`sticky_kernel`] for the HTML/binary arms (the same
//! `nh3.clean` transcription as `api::space::sanitize`, which this crate
//! cannot depend on without a dependency cycle — verified byte-identical
//! configs; the D-24 sticky port reuses it the same way); table names follow
//! `pidash_db::app_issues::models_core` (PIDASHCONV-644); batching reuses
//! [`crate::tasks_cleanup::versions::chunk_ranges`].
//!
//! Raised details vs wire envelopes (the `ser_extras` convention): the kernel
//! returns the details Python *raises* (bare strings / bare-string dicts,
//! exactly as FX-ISS-01 records them). Live DRF list-wraps them on the way
//! out — `{"field": ["msg"]}`, `{"non_field_errors": ["msg"]}` (verified
//! live; see [`CreateValidateError::wire_body`]) — and the handlers layer
//! owns that envelope.
//!
//! Dead arms (verified live; ported faithfully, handler never supplies them):
//!
//! * `description_binary` is a read-only `ModelField` (DRF maps
//!   `BinaryField.editable=False` there), so client input never reaches the
//!   `:332-335` arm nor the sync-lock `description_binary` arm.
//! * The `:280-281` unknown-executor arm fires only for `""`: any other
//!   unknown value dies at the `ChoiceField` with its own message.
//! * [`validate_complexity_score`]'s custom message never fires: the model
//!   `Min/MaxValueValidator(0/10)` run first at field level, and `None` is
//!   rejected by `null=False`.
//! * `attrs["project"]` never exists in `validate` (`project` is read-only
//!   and every call site saves bare), so the project-resolution `attrs` legs
//!   in the pod (`:251-253`) and executor (`:295`) arms never engage.
//!
//! Ported bugs / quirks (translate, don't redesign; also listed in the PR):
//!
//! * `to_representation` echoes `initial_data` ids, not DB rows, so the
//!   default-assignee fallback is invisible in `assignee_ids` (but visible
//!   in the `assignees` m2m key).
//! * The assignee/label project filters silently drop invalid ids.
//! * `create` swallows `IntegrityError` around the *whole* `bulk_create`
//!   call: one bad row aborts its batch and every later batch.
//! * `update` soft-deletes (not hard-deletes) all m2m rows when the key is
//!   present, even for an explicit `[]` clear — but leaves them untouched
//!   when the key is absent.
//! * Missing context keys are an unhandled `KeyError` (500); see
//!   [`CreateValidateError::ContextMissing`] and
//!   [`REQUIRED_CREATE_CONTEXT_KEYS`].
//! * The sync-lock multi-field error dict iterates a *set* intersection, so
//!   its key order is hash-randomized per process in Django; the port emits
//!   [`LOCKED_ISSUE_FIELDS`] order (deterministic). Single-field bodies are
//!   byte-identical.
//! * `description_json` equality is `serde_json::Value ==`, which disagrees
//!   with Python `dict ==` only on mixed int/float numbers (`1 == 1.0` in
//!   Python, unequal as JSON numbers).
//!
//! Fixture: `rust-api/fixtures/app_issues/serializers/FX-ISS-01.create.json`.

use std::borrow::Cow;

use base64::Engine as _;
use serde::Serialize;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use pidash_types::v1_assets::sticky as sticky_kernel;

use super::serializers_engage::issue_is_actively_synced;
use crate::dispatch::{
    cloud_agent_is_configured, managed_runner_availability, LlmProfile, UserFlags,
};
use crate::tasks_cleanup::versions::chunk_ranges;

// ---------------------------------------------------------------------------
// to_representation (issue.py:185-191)
// ---------------------------------------------------------------------------

/// `IssueCreateSerializer.to_representation` wire keys in output order
/// (verified live on the pinned Django + DRF): the 39 readable fields in
/// `get_fields()` order, then the `to_representation` echo appends
/// `assignee_ids` first and `label_ids` second (`:187-190`). The write-only
/// pair is absent from `super().to_representation` — the echo keys sit at
/// the END, not in declared position.
pub const ISSUE_CREATE_FIELDS: [&str; 41] = [
    "id",
    "state_id",
    "parent_id",
    "assigned_pod_id",
    "project_id",
    "workspace_id",
    "created_at",
    "updated_at",
    "deleted_at",
    "point",
    "name",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "git_work_branch",
    "created_via",
    "agent_executor",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "parent",
    "state",
    "estimate_point",
    "type",
    "assignees",
    "labels",
    "assignee_ids",
    "label_ids",
];

/// A database row for `IssueCreateSerializer` rendering. UUID and FK primary
/// keys are pre-rendered strings (`PrimaryKeyRelatedField`, read-only side);
/// datetimes and dates cross this boundary already rendered as DRF iso-8601
/// strings (formatting belongs to the DB edge), so rendering here is a
/// byte-exact passthrough. `sort_order` is a plain `f64` (the merged D-26
/// refs pattern; `serde_json` renders shortest-round-trip like DRF).
/// `assignees` / `labels` are the m2m id lists in DB order.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueCreateRow<'a> {
    pub id: &'a str,
    pub state_id: Option<&'a str>,
    pub parent_id: Option<&'a str>,
    pub assigned_pod_id: Option<&'a str>,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub point: Option<i32>,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub description_binary: Option<&'a [u8]>,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub is_draft: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub git_work_branch: &'a str,
    pub created_via: Option<&'a str>,
    pub agent_executor: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub parent: Option<&'a str>,
    pub state: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub issue_type: Option<&'a str>,
    pub assignees: &'a [&'a str],
    pub labels: &'a [&'a str],
}

/// `IssueCreateSerializer.to_representation` output (`issue.py:185-191`).
/// `description_binary` is the base64 ASCII string — DRF maps `BinaryField`
/// to the generic `ModelField`, whose rendering calls
/// `BinaryField.value_to_string`, i.e. base64 (verified live:
/// `b"<p>hi</p>"` renders `"PHA+aGk8L3A+"`; same rule as the merged assoc
/// port). `assignee_ids` / `label_ids` are the `initial_data` echo.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueCreateView {
    pub id: String,
    pub state_id: Option<String>,
    pub parent_id: Option<String>,
    pub assigned_pod_id: Option<String>,
    pub project_id: String,
    pub workspace_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub point: Option<i32>,
    pub name: String,
    pub description_json: serde_json::Value,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub description_binary: Option<String>,
    pub priority: String,
    pub complexity_score: i32,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<String>,
    pub archived_at: Option<String>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub git_work_branch: String,
    pub created_via: Option<String>,
    pub agent_executor: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub project: String,
    pub workspace: String,
    pub parent: Option<String>,
    pub state: Option<String>,
    pub estimate_point: Option<String>,
    #[serde(rename = "type")]
    pub issue_type: Option<String>,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
    pub assignee_ids: serde_json::Value,
    pub label_ids: serde_json::Value,
}

/// Python truthiness over a JSON-decoded value (`:188-190`'s `if x else []`).
/// `initial_data` for JSON requests holds parsed JSON, so falsy is exactly
/// `null` / `false` / numeric zero / `""` / `[]` / `{}`.
fn initial_value_is_falsy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Bool(flag) => !flag,
        serde_json::Value::Number(number) => {
            number.as_i64() == Some(0) || number.as_u64() == Some(0) || number.as_f64() == Some(0.0)
        }
        serde_json::Value::String(text) => text.is_empty(),
        serde_json::Value::Array(items) => items.is_empty(),
        serde_json::Value::Object(map) => map.is_empty(),
    }
}

/// The `initial_data` echo (`:187-190`): the request's value when present
/// and truthy, otherwise `[]`. Missing key and falsy value both yield `[]`.
pub fn echo_initial_ids(initial: Option<&serde_json::Value>) -> serde_json::Value {
    match initial {
        Some(value) if !initial_value_is_falsy(value) => value.clone(),
        _ => serde_json::Value::Array(Vec::new()),
    }
}

/// Port of `IssueCreateSerializer.to_representation` (`issue.py:185-191`).
/// `initial_assignee_ids` / `initial_label_ids` are the request's raw values
/// (`None` when the key was absent). Field-for-field copy in
/// [`ISSUE_CREATE_FIELDS`] order.
pub fn issue_create_to_representation(
    row: &IssueCreateRow<'_>,
    initial_assignee_ids: Option<&serde_json::Value>,
    initial_label_ids: Option<&serde_json::Value>,
) -> IssueCreateView {
    IssueCreateView {
        id: row.id.to_owned(),
        state_id: row.state_id.map(str::to_owned),
        parent_id: row.parent_id.map(str::to_owned),
        assigned_pod_id: row.assigned_pod_id.map(str::to_owned),
        project_id: row.project_id.to_owned(),
        workspace_id: row.workspace_id.to_owned(),
        created_at: row.created_at.to_owned(),
        updated_at: row.updated_at.to_owned(),
        deleted_at: row.deleted_at.map(str::to_owned),
        point: row.point,
        name: row.name.to_owned(),
        description_json: row.description_json.clone(),
        description_html: row.description_html.to_owned(),
        description_stripped: row.description_stripped.map(str::to_owned),
        description_binary: row
            .description_binary
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
        priority: row.priority.to_owned(),
        complexity_score: row.complexity_score,
        start_date: row.start_date.map(str::to_owned),
        target_date: row.target_date.map(str::to_owned),
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        completed_at: row.completed_at.map(str::to_owned),
        archived_at: row.archived_at.map(str::to_owned),
        is_draft: row.is_draft,
        external_source: row.external_source.map(str::to_owned),
        external_id: row.external_id.map(str::to_owned),
        git_work_branch: row.git_work_branch.to_owned(),
        created_via: row.created_via.map(str::to_owned),
        agent_executor: row.agent_executor.map(str::to_owned),
        created_by: row.created_by.map(str::to_owned),
        updated_by: row.updated_by.map(str::to_owned),
        project: row.project.to_owned(),
        workspace: row.workspace.to_owned(),
        parent: row.parent.map(str::to_owned),
        state: row.state.map(str::to_owned),
        estimate_point: row.estimate_point.map(str::to_owned),
        issue_type: row.issue_type.map(str::to_owned),
        assignees: row.assignees.iter().map(|id| (*id).to_owned()).collect(),
        labels: row.labels.iter().map(|id| (*id).to_owned()).collect(),
        assignee_ids: echo_initial_ids(initial_assignee_ids),
        label_ids: echo_initial_ids(initial_label_ids),
    }
}

// ---------------------------------------------------------------------------
// validate_complexity_score (issue.py:193-204)
// ---------------------------------------------------------------------------

/// `validate_complexity_score` message (`:201-203`).
pub const COMPLEXITY_SCORE_MESSAGE: &str =
    "complexity_score must be an integer from 0 to 10 (0 means unrated).";

/// Port of `validate_complexity_score` (`:193-204`): `None` passes through,
/// `0..=10` passes, anything else is a field error. Dead for request input
/// (verified live): the model `Min/MaxValueValidator(0/10)` run first at
/// field level and `None` is rejected by `null=False` — ported faithfully.
pub fn validate_complexity_score(value: Option<i32>) -> Result<Option<i32>, CreateValidateError> {
    match value {
        None => Ok(None),
        Some(score) if (0..=10).contains(&score) => Ok(Some(score)),
        Some(_) => Err(CreateValidateError::Field {
            field: "complexity_score",
            message: Cow::Borrowed(COMPLEXITY_SCORE_MESSAGE),
        }),
    }
}

// ---------------------------------------------------------------------------
// validate (issue.py:206-385): messages
// ---------------------------------------------------------------------------

/// Sync-locked fields (`_LOCKED_ISSUE_FIELDS`, `:59`), in the order the port
/// reports them. Django iterates a *set* intersection here, so multi-field
/// bodies are hash-ordered per process; single-field bodies are byte-exact.
pub const LOCKED_ISSUE_FIELDS: [&str; 5] = [
    "name",
    "description_html",
    "description_json",
    "description_stripped",
    "description_binary",
];

/// Per-field sync-lock message (`:225-226`).
pub const SYNC_LOCKED_MESSAGE: &str = "This field is synced from a Git provider and is read-only. \
     Unbind the project's repository to edit.";

/// Non-field date message (`:236`).
pub const DATES_MESSAGE: &str = "Start date cannot exceed target date";

/// Pod messages (`:255`, `:257`, `:268`).
pub const POD_DIFFERENT_PROJECT_MESSAGE: &str = "pod is in a different project";
pub const POD_DELETED_MESSAGE: &str = "pod has been deleted";
pub const POD_REASSIGN_MESSAGE: &str = "cannot reassign pod while the issue has an active run";

/// Executor messages (`:281`, `:286`, `:301`, `:320`).
pub const EXECUTOR_UNKNOWN_MESSAGE: &str = "unknown agent executor";
pub const EXECUTOR_CLOUD_UNAVAILABLE_MESSAGE: &str =
    "Pi Dash Cloud Agent is not available on this instance";
pub const EXECUTOR_PROJECT_REQUIRED_MESSAGE: &str = "A project is required for desktop runs";
pub const EXECUTOR_MID_FLIGHT_MESSAGE: &str =
    "cannot change the execution target while the issue has an active run";

/// Managed-runner refusal copy (`_MANAGED_UNAVAILABLE_DETAIL`, `:93-102`),
/// keyed by [`ManagedRunnerReason`] code. Unknown codes fall back to the raw
/// code (`.get(reason, reason)`).
pub fn managed_unavailable_detail(reason: &str) -> Cow<'_, str> {
    if reason == ManagedRunnerReason::DISABLED {
        Cow::Borrowed("Pi Dash Agent is not enabled on this instance")
    } else if reason == ManagedRunnerReason::NOT_CONNECTED {
        Cow::Borrowed("Open the Pi Dash desktop app to run on this computer")
    } else if reason == ManagedRunnerReason::LLM_CONFIG_MISSING {
        Cow::Borrowed("Configure an AI provider in Pi Dash AI settings first")
    } else if reason == ManagedRunnerReason::GATEWAY_SCOPES_MISSING {
        Cow::Borrowed("Sign in to Pi Dash again to refresh your AI access")
    } else if reason == ManagedRunnerReason::BYOK_UNSUPPORTED {
        Cow::Borrowed(
            "Pi Dash Agent on desktop uses OpenHub. Switch your AI provider to OpenHub to run here; \
             Pi Dash AI and the Cloud Agent keep using your own key.",
        )
    } else {
        Cow::Owned(reason.to_owned())
    }
}

/// State / parent / estimate non-field messages (`:364`, `:374`, `:383`).
pub const STATE_INVALID_MESSAGE: &str = "State is not valid please pass a valid state_id";
pub const PARENT_INVALID_MESSAGE: &str =
    "Parent is not valid issue_id please pass a valid issue_id";
pub const ESTIMATE_INVALID_MESSAGE: &str =
    "Estimate point is not valid please pass a valid estimate_point_id";

/// The `validate()` failure: the detail Python *raises* (bare-string form,
/// exactly as FX-ISS-01 records it). [`CreateValidateError::wire_body`]
/// applies DRF's mechanical list-wrap for tests; the handlers layer owns
/// the live envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateValidateError {
    /// `ValidationError({"field": "msg"})` (`:255`, `:309`, …).
    Field {
        field: &'static str,
        message: Cow<'static, str>,
    },
    /// `ValidationError({f: msg, …})` — the sync lock (`:223-229`), in
    /// [`LOCKED_ISSUE_FIELDS`] order.
    Fields(Vec<(&'static str, Cow<'static, str>)>),
    /// `ValidationError("msg")` — dates, state, parent, estimate.
    NonField { message: &'static str },
    /// `self.context["project_id"]` with the key absent (`:339-341`): an
    /// unhandled `KeyError`, i.e. a 500 with Django's HTML error page — no
    /// JSON body exists. The handler must answer 500.
    ContextMissing { key: &'static str },
}

impl CreateValidateError {
    /// The raised detail: `{"field": "msg"}` / `{"f": "msg", …}` / `"msg"`.
    /// `None` for [`CreateValidateError::ContextMissing`] (a 500, no body).
    pub fn raised_detail(&self) -> Option<serde_json::Value> {
        match self {
            CreateValidateError::Field { field, message } => {
                let mut map = serde_json::Map::with_capacity(1);
                map.insert(
                    (*field).to_owned(),
                    serde_json::Value::String(message.to_string()),
                );
                Some(serde_json::Value::Object(map))
            }
            CreateValidateError::Fields(entries) => {
                let map: serde_json::Map<String, serde_json::Value> = entries
                    .iter()
                    .map(|(field, message)| {
                        (
                            (*field).to_owned(),
                            serde_json::Value::String(message.to_string()),
                        )
                    })
                    .collect();
                Some(serde_json::Value::Object(map))
            }
            CreateValidateError::NonField { message } => {
                Some(serde_json::Value::String((*message).to_owned()))
            }
            CreateValidateError::ContextMissing { .. } => None,
        }
    }

    /// The live wire body: DRF list-wraps every raised detail —
    /// `{"field": ["msg"]}`, `{"non_field_errors": ["msg"]}` (verified live).
    /// `None` for [`CreateValidateError::ContextMissing`] (a 500, no body).
    pub fn wire_body(&self) -> Option<serde_json::Value> {
        match self {
            CreateValidateError::Field { field, message } => {
                let mut map = serde_json::Map::with_capacity(1);
                map.insert(
                    (*field).to_owned(),
                    serde_json::Value::Array(vec![serde_json::Value::String(message.to_string())]),
                );
                Some(serde_json::Value::Object(map))
            }
            CreateValidateError::Fields(entries) => {
                let map: serde_json::Map<String, serde_json::Value> = entries
                    .iter()
                    .map(|(field, message)| {
                        (
                            (*field).to_owned(),
                            serde_json::Value::Array(vec![serde_json::Value::String(
                                message.to_string(),
                            )]),
                        )
                    })
                    .collect();
                Some(serde_json::Value::Object(map))
            }
            CreateValidateError::NonField { message } => {
                Some(serde_json::json!({ "non_field_errors": [message] }))
            }
            CreateValidateError::ContextMissing { .. } => None,
        }
    }

    /// Whether this failure is a 500 (`KeyError` parity), not a 400.
    pub fn is_server_error(&self) -> bool {
        matches!(self, CreateValidateError::ContextMissing { .. })
    }
}

// ---------------------------------------------------------------------------
// validate (issue.py:206-385): inputs
// ---------------------------------------------------------------------------

/// The resolved pod `validate()` reads (`:244-269`): field validation
/// already resolved `assigned_pod_id` through `Pod.all_objects` (tombstones
/// included, so the friendly deleted message can fire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PodRef {
    /// `pod.id`.
    pub id: uuid::Uuid,
    /// `pod.project_id`.
    pub project_id: uuid::Uuid,
    /// Whether `pod.deleted_at is not None`.
    pub deleted: bool,
}

/// The project `validate()` resolves for the managed arm (`:295-301`) and
/// the pod project check (`:247-254`): exactly the consumed columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedProject {
    /// `project.id`.
    pub project_id: uuid::Uuid,
    /// `project.workspace_id` (consumed by the runner-enrollment probes).
    pub workspace_id: uuid::Uuid,
}

/// The instance-side locked values for the sync lock (`:216-229`).
/// `description_binary` needs no instance value: any present attr always
/// differs (`str != bytes` in Python), so presence alone blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct LockedIssueValues<'a> {
    /// `instance.name`.
    pub name: &'a str,
    /// `instance.description_html`.
    pub description_html: &'a str,
    /// `instance.description_json`.
    pub description_json: &'a serde_json::Value,
    /// `instance.description_stripped` (nullable).
    pub description_stripped: Option<&'a str>,
}

/// The attrs-side locked values: `None` means the key is absent from attrs
/// (only present keys are checked). `description_html` is the RAW pre-
/// sanitize input — the lock runs at step 1, sanitize at step 5.
#[derive(Debug, Clone, PartialEq)]
pub struct LockedIssueAttrs<'a> {
    /// `attrs["name"]` when the key is present.
    pub name: Option<&'a str>,
    /// Raw `attrs["description_html"]` when the key is present.
    pub description_html: Option<&'a str>,
    /// `attrs["description_json"]` when the key is present.
    pub description_json: Option<&'a serde_json::Value>,
    /// `attrs["description_stripped"]`: outer `None` = key absent, inner
    /// `None` = explicit null (the field allows null).
    pub description_stripped: Option<Option<&'a str>>,
    /// Whether `description_binary` is present in attrs (always blocks;
    /// unreachable through DRF input — read-only `ModelField` drops it).
    pub description_binary_present: bool,
}

/// Post-field-validation attrs consumed by `validate()` (`:206-385`).
/// `None` = the key is absent from attrs (partial update / create default).
#[derive(Debug, Clone, PartialEq)]
pub struct ValidateAttrs<'a> {
    /// Locked-field attrs for the sync lock (step 1).
    pub locked: LockedIssueAttrs<'a>,
    /// `attrs["start_date"]` / `attrs["target_date"]` (step 2).
    pub start_date: Option<chrono::NaiveDate>,
    /// `attrs["target_date"]` (step 2).
    pub target_date: Option<chrono::NaiveDate>,
    /// `attrs["assigned_pod"]` (step 3): outer `None` = key absent,
    /// `Some(None)` = explicit clear, `Some(Some)` = resolved pod.
    pub assigned_pod: Option<Option<PodRef>>,
    /// `attrs["agent_executor"]` (step 4): outer `None` = key absent,
    /// `Some(None)` = clear the override, `Some(Some)` = pinned value.
    pub agent_executor: Option<Option<&'a str>>,
    /// `attrs["description_html"]` (step 5; truthy inputs only).
    pub description_html: Option<&'a str>,
    /// `attrs["description_binary"]` base64 (step 6; dead through DRF).
    pub description_binary: Option<&'a str>,
    /// `attrs["assignee_ids"]` resolved user ids (step 7).
    pub assignee_ids: Option<&'a [uuid::Uuid]>,
    /// `attrs["label_ids"]` resolved label ids (step 8).
    pub label_ids: Option<&'a [uuid::Uuid]>,
    /// `attrs["state"].id` (step 9).
    pub state: Option<uuid::Uuid>,
    /// `attrs["parent"].id` (step 10).
    pub parent: Option<uuid::Uuid>,
    /// `attrs["estimate_point"].id` (step 11).
    pub estimate_point: Option<uuid::Uuid>,
    /// `attrs["project"]` for the pod (`:251-253`) and managed (`:295`)
    /// project-resolution legs. Dead in practice (`project` is read-only
    /// and every call site saves bare) — modeled for fidelity.
    pub attrs_project: Option<ResolvedProject>,
}

/// The `validate()` context slice (`:207`, `:247`, `:296-299`, `:339`, `:351`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidateContext {
    /// `context["project_id"]` / `context.get("project_id")`: `None` covers
    /// both a missing key (the assignee arm's `KeyError`) and a `None`
    /// value (the label arm's `IS NULL` tolerant path).
    pub project_id: Option<uuid::Uuid>,
    /// `context.get("allow_triage_state", False)` (`:207`).
    pub allow_triage_state: bool,
}

/// The update-path instance snapshot (`self.instance is not None`).
#[derive(Debug, Clone, PartialEq)]
pub struct ValidateInstance<'a> {
    /// `instance.external_source` for
    /// [`issue_is_actively_synced`](super::serializers_engage::issue_is_actively_synced).
    pub external_source: Option<&'a str>,
    /// The `is_synced` annotation when the queryset carries it.
    pub annotated_is_synced: Option<bool>,
    /// Current locked-field values.
    pub locked: LockedIssueValues<'a>,
    /// `instance.project_id` (pod project-resolution leg, `:248-249`).
    pub project_id: uuid::Uuid,
    /// `instance.project` as [`ResolvedProject`] (managed leg, `:295`):
    /// the FK is non-nullable, so this always resolves on live rows.
    pub project: ResolvedProject,
    /// `instance.assigned_pod_id` (`:264`).
    pub assigned_pod_id: Option<uuid::Uuid>,
    /// `instance.agent_executor` (`:316`; `""` impossible on live rows but
    /// comparable all the same).
    pub agent_executor: Option<&'a str>,
}

/// Executor-policy inputs for the cloud/managed arms (`:284-310`).
/// `PartialEq` only: the settings structs implement `PartialEq`, not `Eq`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecutorPolicy<'a> {
    /// `CLOUD_AGENT_ENABLED` for [`cloud_agent_is_configured`].
    pub cloud: &'a CloudAgentSettings,
    /// `MANAGED_RUNNER_ENABLED` for [`managed_runner_availability`].
    pub managed: &'a ManagedRunnerSettings,
    /// `request.user` flags (`None` = anonymous / absent request).
    pub viewer: Option<&'a UserFlags>,
}

/// Read probes for `validate()`. Plain `&dyn Fn` (pure reads): each runs at
/// most once and only when its arm is reached, so probe counts match
/// Django's — except `has_active_run`, which Python evaluates per access
/// (pod arm `:266` AND executor arm `:317`), hence repeatable `Fn`, and
/// the sync probes, which run only inside
/// [`issue_is_actively_synced`](super::serializers_engage::issue_is_actively_synced)'s
/// short-circuit. The handler implements each with the documented `*_SQL`.
#[derive(Clone, Copy)]
pub struct ValidateProbes<'a> {
    /// [`GIT_ISSUE_SYNC_PROBE_SQL`](super::serializers_engage::GIT_ISSUE_SYNC_PROBE_SQL)
    /// verdict.
    pub git_sync_exists: &'a dyn Fn() -> bool,
    /// [`GITHUB_ISSUE_SYNC_PROBE_SQL`](super::serializers_engage::GITHUB_ISSUE_SYNC_PROBE_SQL)
    /// verdict.
    pub github_sync_exists: &'a dyn Fn() -> bool,
    /// [`HAS_ACTIVE_RUN_SQL`] verdict. Called up to twice (pod + executor
    /// mid-flight checks), exactly like the `has_active_run` property.
    pub has_active_run: &'a dyn Fn() -> bool,
    /// `ProjectMember` filter (`:338-344`): [`assignee_member_filter_sql`]
    /// over the input ids, `-created_at` order.
    pub filter_assignees: &'a dyn Fn(&[uuid::Uuid]) -> Vec<uuid::Uuid>,
    /// `Label` filter (`:347-354`): [`label_filter_sql`] (or
    /// [`label_filter_null_project_sql`] when the context project is
    /// `None`), `-created_at` order.
    pub filter_labels: &'a dyn Fn(&[uuid::Uuid]) -> Vec<uuid::Uuid>,
    /// [`STATE_EXISTS_SQL`] verdict (default manager).
    pub state_exists: &'a dyn Fn() -> bool,
    /// [`STATE_TRIAGE_EXISTS_SQL`] verdict (`allow_triage_state`).
    pub triage_state_exists: &'a dyn Fn() -> bool,
    /// [`PARENT_EXISTS_SQL`] verdict.
    pub parent_exists: &'a dyn Fn() -> bool,
    /// [`ESTIMATE_EXISTS_SQL`] verdict.
    pub estimate_exists: &'a dyn Fn() -> bool,
    /// [`PROJECT_FETCH_SQL`] verdict for the managed arm (`:296-299`):
    /// runs only when neither the instance nor attrs resolved a project.
    pub fetch_project: &'a dyn Fn() -> Option<ResolvedProject>,
    /// The `managed_llm_profile` seam input to
    /// [`managed_runner_availability`].
    pub llm_profile: &'a dyn Fn() -> LlmProfile,
    /// [`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`](crate::dispatch::ENROLLED_MANAGED_RUNNERS_EXISTS_SQL)
    /// verdict.
    pub enrolled_exists: &'a dyn Fn() -> bool,
    /// [`ONLINE_MANAGED_RUNNER_SQL`](crate::dispatch::ONLINE_MANAGED_RUNNER_SQL)
    /// verdict.
    pub online_exists: &'a dyn Fn() -> bool,
}

/// The attrs `validate()` mutates, returned on success. Every other key
/// passes through untouched (the handler keeps its own map): only
/// `description_html` is replaced (sanitized, `:329-330`) and the id lists
/// filtered (`:339-354`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedAttrs {
    /// Cleaned HTML when the input key was present and truthy.
    pub description_html: Option<String>,
    /// Surviving member ids when the input was present and non-empty.
    pub assignee_ids: Option<Vec<uuid::Uuid>>,
    /// Surviving label ids when the input was present and non-empty.
    pub label_ids: Option<Vec<uuid::Uuid>>,
}

/// The sync-lock arm (`:216-229`): fields present in attrs whose value
/// differs from the instance, in [`LOCKED_ISSUE_FIELDS`] order. A present
/// binary attr always blocks (`str != bytes` in Python).
fn sync_locked_blocked<'a>(
    attrs: &LockedIssueAttrs<'a>,
    instance: &LockedIssueValues<'a>,
) -> Vec<&'static str> {
    let mut blocked = Vec::new();
    if attrs.name.is_some_and(|value| value != instance.name) {
        blocked.push("name");
    }
    if attrs
        .description_html
        .is_some_and(|value| value != instance.description_html)
    {
        blocked.push("description_html");
    }
    if attrs
        .description_json
        .is_some_and(|value| value != instance.description_json)
    {
        blocked.push("description_json");
    }
    if attrs
        .description_stripped
        .is_some_and(|value| value != instance.description_stripped)
    {
        blocked.push("description_stripped");
    }
    if attrs.description_binary_present {
        blocked.push("description_binary");
    }
    blocked
}

/// Port of `IssueCreateSerializer.validate` (`:206-385`): the 11 ordered
/// steps, first failure wins. `instance` is `None` on create. Probes run
/// only when reached (see [`ValidateProbes`]).
pub fn issue_create_validate(
    attrs: &ValidateAttrs<'_>,
    ctx: &ValidateContext,
    instance: Option<&ValidateInstance<'_>>,
    policy: &ExecutorPolicy<'_>,
    probes: &ValidateProbes<'_>,
) -> Result<ValidatedAttrs, CreateValidateError> {
    // Step 1. Sync lock (:216-229).
    if let Some(current) = instance {
        if issue_is_actively_synced(
            current.external_source,
            current.annotated_is_synced,
            || (probes.git_sync_exists)(),
            || (probes.github_sync_exists)(),
        ) {
            let blocked = sync_locked_blocked(&attrs.locked, &current.locked);
            if !blocked.is_empty() {
                return Err(CreateValidateError::Fields(
                    blocked
                        .into_iter()
                        .map(|field| (field, Cow::Borrowed(SYNC_LOCKED_MESSAGE)))
                        .collect(),
                ));
            }
        }
    }

    // Step 2. Start/target dates (:231-236): non-field error.
    if let (Some(start), Some(target)) = (attrs.start_date, attrs.target_date) {
        if start > target {
            return Err(CreateValidateError::NonField {
                message: DATES_MESSAGE,
            });
        }
    }

    // Step 3. assigned_pod (:244-269): key presence, not truthiness.
    if let Some(pod) = attrs.assigned_pod {
        if let Some(pod) = pod {
            let project_id = ctx
                .project_id
                .or_else(|| instance.map(|current| current.project_id))
                .or_else(|| attrs.attrs_project.map(|project| project.project_id));
            if project_id.is_some_and(|expected| pod.project_id != expected) {
                return Err(CreateValidateError::Field {
                    field: "assigned_pod_id",
                    message: Cow::Borrowed(POD_DIFFERENT_PROJECT_MESSAGE),
                });
            }
            if pod.deleted {
                return Err(CreateValidateError::Field {
                    field: "assigned_pod_id",
                    message: Cow::Borrowed(POD_DELETED_MESSAGE),
                });
            }
        }
        if let Some(current) = instance {
            if current.assigned_pod_id.is_some() {
                let new_pod_id = pod.map(|resolved| resolved.id);
                if new_pod_id != current.assigned_pod_id && (probes.has_active_run)() {
                    return Err(CreateValidateError::Field {
                        field: "assigned_pod_id",
                        message: Cow::Borrowed(POD_REASSIGN_MESSAGE),
                    });
                }
            }
        }
    }

    // Step 4. agent_executor (:275-321): key presence, not truthiness.
    if let Some(executor) = attrs.agent_executor {
        if let Some(executor) = executor {
            let Some(kind) = AgentExecutorKind::from_value(executor) else {
                return Err(CreateValidateError::Field {
                    field: "agent_executor",
                    message: Cow::Borrowed(EXECUTOR_UNKNOWN_MESSAGE),
                });
            };
            if kind == AgentExecutorKind::CloudAgent && !cloud_agent_is_configured(policy.cloud) {
                return Err(CreateValidateError::Field {
                    field: "agent_executor",
                    message: Cow::Borrowed(EXECUTOR_CLOUD_UNAVAILABLE_MESSAGE),
                });
            }
            if kind == AgentExecutorKind::ManagedRunner {
                let project = instance
                    .map(|current| current.project)
                    .or(attrs.attrs_project)
                    .or_else(|| (probes.fetch_project)());
                let Some(_project) = project else {
                    return Err(CreateValidateError::Field {
                        field: "agent_executor",
                        message: Cow::Borrowed(EXECUTOR_PROJECT_REQUIRED_MESSAGE),
                    });
                };
                let verdict = managed_runner_availability(
                    policy.managed,
                    policy.viewer,
                    || (probes.llm_profile)(),
                    (probes.enrolled_exists)(),
                    (probes.online_exists)(),
                );
                if !verdict.available
                    && verdict.reason_code != ManagedRunnerReason::NO_RUNNER_FOR_PROJECT
                {
                    return Err(CreateValidateError::Field {
                        field: "agent_executor",
                        message: managed_unavailable_detail(&verdict.reason_code)
                            .into_owned()
                            .into(),
                    });
                }
            }
        }
        if let Some(current) = instance {
            if executor != current.agent_executor && (probes.has_active_run)() {
                return Err(CreateValidateError::Field {
                    field: "agent_executor",
                    message: Cow::Borrowed(EXECUTOR_MID_FLIGHT_MESSAGE),
                });
            }
        }
    }

    let mut validated = ValidatedAttrs {
        description_html: None,
        assignee_ids: None,
        label_ids: None,
    };

    // Step 5. description_html sanitize (:324-330): truthy inputs only; the
    // cleaned output always replaces the input (`nh3.clean` never returns
    // `None` — it raises, which is the invalid arm).
    if let Some(html) = attrs.description_html {
        if !html.is_empty() {
            match sticky_kernel::sanitize_html(html) {
                sticky_kernel::SanitizeOutcome::Clean(clean) => {
                    validated.description_html = Some(clean);
                }
                sticky_kernel::SanitizeOutcome::Invalid => {
                    return Err(CreateValidateError::Field {
                        field: "error",
                        message: Cow::Borrowed(sticky_kernel::HTML_INVALID_MESSAGE),
                    });
                }
            }
        }
    }

    // Step 6. description_binary (:332-335): truthy inputs only; the
    // validator's own message is discarded for the literal.
    if let Some(blob) = attrs.description_binary {
        if !blob.is_empty() && sticky_kernel::validate_binary_b64(blob).is_err() {
            return Err(CreateValidateError::Field {
                field: "description_binary",
                message: Cow::Borrowed(sticky_kernel::BINARY_INVALID_MESSAGE),
            });
        }
    }

    // Step 7. Assignee project filter (:338-344): silent drop, `-created_at`
    // order. `context["project_id"]` — a missing key is a `KeyError` (500).
    if let Some(ids) = attrs.assignee_ids {
        if !ids.is_empty() {
            if ctx.project_id.is_none() {
                return Err(CreateValidateError::ContextMissing { key: "project_id" });
            }
            validated.assignee_ids = Some((probes.filter_assignees)(ids));
        }
    }

    // Step 8. Label project filter (:347-354): silent drop like assignees,
    // but `context.get` (a `None` project filters `IS NULL`).
    if let Some(ids) = attrs.label_ids {
        if !ids.is_empty() {
            validated.label_ids = Some((probes.filter_labels)(ids));
        }
    }

    // Step 9. State project check (:357-364): non-field error. The triage
    // manager admits ONLY triage-group states (it replaces, not extends).
    if attrs.state.is_some() {
        let exists = if ctx.allow_triage_state {
            (probes.triage_state_exists)()
        } else {
            (probes.state_exists)()
        };
        if !exists {
            return Err(CreateValidateError::NonField {
                message: STATE_INVALID_MESSAGE,
            });
        }
    }

    // Step 10. Parent project check (:367-374): non-field error. Despite the
    // comment, the check is same-project — not cross-workspace.
    if attrs.parent.is_some() && !(probes.parent_exists)() {
        return Err(CreateValidateError::NonField {
            message: PARENT_INVALID_MESSAGE,
        });
    }

    // Step 11. Estimate project check (:376-383): non-field error.
    if attrs.estimate_point.is_some() && !(probes.estimate_exists)() {
        return Err(CreateValidateError::NonField {
            message: ESTIMATE_INVALID_MESSAGE,
        });
    }

    Ok(validated)
}

// ---------------------------------------------------------------------------
// Read SQL for validate/create/update probes
// ---------------------------------------------------------------------------

/// `Pod.all_objects.get(pk)` for `assigned_pod_id` field resolution
/// (`:153-155`, `runner/models.py:96`): NO soft-delete filter (tombstones
/// resolve so `validate()` can answer "pod has been deleted"). No `ORDER
/// BY`, no `LIMIT` (`.get()` semantics on a PK). Projection covers exactly
/// the consumed columns (`:254-256`); the full-row fetch is unobservable.
/// Params: `$1` pod id (uuid).
pub const POD_FETCH_SQL: &str = "SELECT id, project_id, deleted_at FROM pod WHERE id = $1";

/// `State.objects.filter(project_id, pk).exists()` (`:357-364`): the default
/// manager excludes soft-deleted AND triage-group rows
/// (`db/models/state.py:64-67`). Django renders the exclusion as
/// `NOT ("group" = 'triage')`, kept verbatim. Params: `$1` project id
/// (uuid), `$2` state id (uuid). A `None` context project renders
/// `IS NULL` (unreachable: every call site passes `project_id`).
pub const STATE_EXISTS_SQL: &str = "SELECT 1 FROM states WHERE project_id = $1 AND id = $2 \
     AND deleted_at IS NULL AND NOT (\"group\" = 'triage') LIMIT 1";

/// `State.triage_objects.filter(project_id, pk).exists()` (`:207-208`,
/// `:357-364`): triage-group rows only, soft-deleted excluded
/// (`db/models/state.py:70-73`). Params: `$1` project id (uuid), `$2` state
/// id (uuid). A `None` context project renders `IS NULL` (unreachable:
/// every call site passes `project_id`).
pub const STATE_TRIAGE_EXISTS_SQL: &str = "SELECT 1 FROM states WHERE project_id = $1 AND id = $2 \
     AND deleted_at IS NULL AND \"group\" = 'triage' LIMIT 1";

/// `Issue.objects.filter(project_id, pk).exists()` (`:367-374`): the default
/// `SoftDeletionManager` (`db/mixins.py:53-58`; `issue_objects` is a
/// separate, stricter manager and is NOT used here). Params: `$1` project
/// id (uuid), `$2` issue id (uuid). A `None` context project renders
/// `IS NULL` (unreachable: every call site passes `project_id`).
pub const PARENT_EXISTS_SQL: &str =
    "SELECT 1 FROM issues WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL LIMIT 1";

/// `EstimatePoint.objects.filter(project_id, pk).exists()` (`:376-383`):
/// default soft-delete guard. Params: `$1` project id (uuid), `$2`
/// estimate-point id (uuid). A `None` context project renders `IS NULL`
/// (unreachable: every call site passes `project_id`).
pub const ESTIMATE_EXISTS_SQL: &str =
    "SELECT 1 FROM estimate_points WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL LIMIT 1";

/// `Project.objects.filter(pk).first()` for the managed arm (`:296-299`):
/// default soft-delete guard; `.first()` applies the model ordering
/// (`-created_at`, `db/models/project.py:254`). Projection covers exactly
/// the consumed columns (`managed_runner_availability` reads `id` +
/// `workspace_id`). Params: `$1` project id (uuid). A `None` context
/// project renders `IS NULL` (no row: a PK is never null).
pub const PROJECT_FETCH_SQL: &str = "SELECT id, workspace_id FROM projects WHERE id = $1 \
     AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1";

/// `AgentRun.objects.filter(work_item, status__in=NON_TERMINAL_STATUSES).exists()`
/// for `has_active_run` (`db/models/issue.py:232-248`,
/// `runner/services/matcher.py:54-66`): plain manager, no soft-delete
/// guard. Statuses are the lowercase `TextChoices` values in
/// `NON_TERMINAL_STATUSES` tuple order (Django renders `IN` in the given
/// order). Params: `$1` issue id (uuid).
pub const HAS_ACTIVE_RUN_SQL: &str =
    "SELECT 1 FROM agent_run WHERE work_item_id = $1 AND status IN \
     ('queued', 'assigned', 'waiting_for_worktree', 'running', 'cancel_requested', \
     'awaiting_approval', 'awaiting_reauth', 'paused_awaiting_input') LIMIT 1";

/// `ProjectMember.objects.filter(member_id=default, project_id, role>=15,
/// is_active).exists()` for the create default-assignee fallback
/// (`:423-430`): default soft-delete guard included. Params: `$1` member id
/// (uuid), `$2` project id (uuid).
pub const DEFAULT_ASSIGNEE_EXISTS_SQL: &str = "SELECT 1 FROM project_members WHERE member_id = $1 \
     AND project_id = $2 AND role >= 15 AND is_active AND deleted_at IS NULL LIMIT 1";

/// Render `$first..$last` placeholders (`$2, $3, …`) for an `IN` list over
/// `count` ids, the codebase's dynamic-list form.
fn in_placeholders(count: usize, first: u32) -> String {
    (0..count)
        .map(|index| format!("${}", first + index as u32))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `ProjectMember.objects.filter(project_id, role__gte=15, is_active,
/// member_id__in=ids).values_list("member_id")` (`:338-344`): the silent
/// drop. Default soft-delete guard; `-created_at` model ordering
/// (`db/models/project.py:377`) applies to the `values_list`. Params: `$1`
/// project id (uuid), `$2..` member ids (uuid). Never called with zero ids
/// (the arm runs on non-empty input only).
pub fn assignee_member_filter_sql(count: usize) -> String {
    format!(
        "SELECT member_id FROM project_members WHERE project_id = $1 AND role >= 15 AND is_active \
         AND member_id IN ({}) AND deleted_at IS NULL ORDER BY created_at DESC",
        in_placeholders(count, 2)
    )
}

/// `Label.objects.filter(project_id, id__in=ids).values_list("id")`
/// (`:347-354`): the silent drop. Default soft-delete guard; `-created_at`
/// model ordering (`db/models/label.py:44`). Params: `$1` project id
/// (uuid), `$2..` label ids (uuid). Never called with zero ids.
pub fn label_filter_sql(count: usize) -> String {
    format!(
        "SELECT id FROM labels WHERE project_id = $1 AND id IN ({}) \
         AND deleted_at IS NULL ORDER BY created_at DESC",
        in_placeholders(count, 2)
    )
}

/// The label arm with a `None` context project (`:351` uses
/// `context.get`): `project_id=None` renders `IS NULL`, matching only
/// workspace-level (project-less) labels. Params: `$1..` label ids (uuid).
pub fn label_filter_null_project_sql(count: usize) -> String {
    format!(
        "SELECT id FROM labels WHERE project_id IS NULL AND id IN ({}) \
         AND deleted_at IS NULL ORDER BY created_at DESC",
        in_placeholders(count, 1)
    )
}

// ---------------------------------------------------------------------------
// create (issue.py:387-462) / update (issue.py:464-518): write specs
// ---------------------------------------------------------------------------

/// Context keys `create()` reads with `[]` (`:391-393`): a missing key is
/// an unhandled `KeyError` (500). Every call site passes all three
/// (`app/views/issue/base.py:400-408`, intake, draft-to-issue).
pub const REQUIRED_CREATE_CONTEXT_KEYS: [&str; 3] =
    ["project_id", "workspace_id", "default_assignee_id"];

/// `bulk_create(..., batch_size=10)` chunk size (`:416`, `:457`, `:489`,
/// `:510`): one multi-row `INSERT` per chunk, input order.
pub const M2M_BATCH_SIZE: usize = 10;

/// Split `ids` into `bulk_create(batch_size=10)` chunks (via the merged
/// [`chunk_ranges`]): each slice is one multi-row `INSERT`, in order.
pub fn m2m_batches(ids: &[uuid::Uuid]) -> Vec<&[uuid::Uuid]> {
    chunk_ranges(ids.len(), M2M_BATCH_SIZE)
        .into_iter()
        .map(|(start, end)| &ids[start..end])
        .collect()
}

/// The m2m `INSERT` column list in `bulk_create` (concrete-field) order:
/// `id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
/// project_id, workspace_id, issue_id, <member>` (matches
/// `models_core::{issue_assignee, issue_label}::COLUMNS`).
pub const M2M_INSERT_COLUMNS: &str = "id, created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, project_id, workspace_id, issue_id";

/// Multi-row `INSERT` for one [`m2m_batches`] chunk of `IssueAssignee` /
/// `IssueLabel` rows (`:404-417`, `:445-459`, `:477-491`, `:499-513`).
/// `member_column` is `assignee_id` or `label_id`. Per-row params (9):
/// id (uuid4), created_at (now), updated_at (now), created_by_id
/// (nullable uuid), updated_by_id (nullable uuid), project_id, workspace_id,
/// issue_id, member id — `deleted_at` is a literal `NULL` (no path sets
/// it). `ignore_conflicts` (update only, `:490`, `:511`) appends Django's
/// bare `ON CONFLICT DO NOTHING`.
pub fn m2m_insert_sql(
    table: &str,
    member_column: &str,
    rows: usize,
    ignore_conflicts: bool,
) -> String {
    let mut sql = format!("INSERT INTO {table} ({M2M_INSERT_COLUMNS}, {member_column}) VALUES ");
    let groups: Vec<String> = (0..rows)
        .map(|row| {
            let base = (row * 9) as u32;
            let placeholders: Vec<String> =
                (1..=9).map(|index| format!("${}", base + index)).collect();
            // $1..$5 then literal NULL for deleted_at, then $6..$9.
            format!(
                "({}, {}, {}, {}, {}, NULL, {}, {}, {}, {})",
                placeholders[0],
                placeholders[1],
                placeholders[2],
                placeholders[3],
                placeholders[4],
                placeholders[5],
                placeholders[6],
                placeholders[7],
                placeholders[8]
            )
        })
        .collect();
    sql.push_str(&groups.join(", "));
    if ignore_conflicts {
        sql.push_str(" ON CONFLICT DO NOTHING");
    }
    sql
}

/// `IssueAssignee.objects.filter(issue).delete()` (`:475`) and the label
/// twin (`:496`): a SOFT delete — `SoftDeletionQuerySet.delete` defaults to
/// `soft=True` (`db/mixins.py:48-52`), i.e. `UPDATE … SET deleted_at`,
/// scoped by the default manager (`deleted_at IS NULL`). `updated_at` is
/// NOT touched (queryset `update()` skips `auto_now`). Params: `$1`
/// deletion timestamp, `$2` issue id (uuid).
pub const ASSIGNEE_CLEAR_SQL: &str =
    "UPDATE issue_assignees SET deleted_at = $1 WHERE issue_id = $2 AND deleted_at IS NULL";
/// Label twin of [`ASSIGNEE_CLEAR_SQL`] (`:496`). Params: `$1` deletion
/// timestamp, `$2` issue id (uuid).
pub const LABEL_CLEAR_SQL: &str =
    "UPDATE issue_labels SET deleted_at = $1 WHERE issue_id = $2 AND deleted_at IS NULL";

/// The create default-assignee fallback (`:420-441`): when the validated
/// (post-filter) assignee list is `None` or empty AND
/// `context["default_assignee_id"]` is `Some` AND that member passes
/// [`DEFAULT_ASSIGNEE_EXISTS_SQL`], one `IssueAssignee` row is inserted
/// (single-row [`m2m_insert_sql`], `IntegrityError` swallowed). Returns the
/// member id to insert, if any.
pub fn create_default_assignee_fallback(
    assignees: Option<&[uuid::Uuid]>,
    default_assignee_id: Option<uuid::Uuid>,
    default_is_valid_member: bool,
) -> Option<uuid::Uuid> {
    let empty = assignees.is_none_or(<[uuid::Uuid]>::is_empty);
    if empty {
        if let Some(member) = default_assignee_id {
            if default_is_valid_member {
                return Some(member);
            }
        }
    }
    None
}

/// `create()` (`:387-462`) execution contract for the handlers layer, in
/// order: (1) require [`REQUIRED_CREATE_CONTEXT_KEYS`] (missing key = 500);
/// (2) `Issue.objects.create(**validated_data, project_id)` — the row write
/// plus `Issue.save()` halves (state default, sequence, sort order,
/// `description_stripped`) belong to the models layer (FX-ISS-07) and the
/// `pre/post_save` signals to the tasks layer (FX-ISS-21); (3) assignees:
/// [`m2m_batches`] inserts via [`m2m_insert_sql`] WITHOUT `ignore_conflicts`
/// when the validated list is non-empty, else
/// [`create_default_assignee_fallback`] — the whole `bulk_create` call sits
/// inside ONE `try/except IntegrityError: pass`, so the first failing batch
/// aborts the rest and the error is swallowed; (4) labels: same, no
/// fallback. `created_by_id` / `updated_by_id` on every m2m row come from
/// the freshly saved issue (crum current user at save time).
pub const CREATE_CONTRACT: &str = "context keys, issue row (models), assignee batches-or-fallback, label batches; one swallowed IntegrityError scope per bulk_create";

/// `update()` (`:464-518`) execution contract: (1) when `assignee_ids` is
/// present (even `[]`): [`ASSIGNEE_CLEAR_SQL`], then [`m2m_batches`] inserts
/// via [`m2m_insert_sql`] WITH `ignore_conflicts`, `IntegrityError`
/// swallowed; absent key leaves rows untouched; (2) same for `label_ids`
/// with [`LABEL_CLEAR_SQL`]; (3) `instance.updated_at = now()` runs BEFORE
/// the issue-row `UPDATE` even when only m2m keys changed (`:517-518`) —
/// the row write always carries a fresh `updated_at`.
pub const UPDATE_CONTRACT: &str = "per present m2m key: soft-clear then ignore-conflicts batches (swallowed); updated_at bumped before the issue UPDATE even for m2m-only writes";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::cell::Cell;
    use std::rc::Rc;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_issues/serializers/FX-ISS-01.create.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }

    fn project() -> ResolvedProject {
        ResolvedProject {
            project_id: uid(1),
            workspace_id: uid(2),
        }
    }

    fn locked_attrs_none() -> LockedIssueAttrs<'static> {
        LockedIssueAttrs {
            name: None,
            description_html: None,
            description_json: None,
            description_stripped: None,
            description_binary_present: false,
        }
    }

    fn base_attrs() -> ValidateAttrs<'static> {
        ValidateAttrs {
            locked: locked_attrs_none(),
            start_date: None,
            target_date: None,
            assigned_pod: None,
            agent_executor: None,
            description_html: None,
            description_binary: None,
            assignee_ids: None,
            label_ids: None,
            state: None,
            parent: None,
            estimate_point: None,
            attrs_project: None,
        }
    }

    fn locked_values<'a>(json: &'a Value) -> LockedIssueValues<'a> {
        LockedIssueValues {
            name: "Old",
            description_html: "<p>old</p>",
            description_json: json,
            description_stripped: Some("old"),
        }
    }

    fn instance<'a>(locked: LockedIssueValues<'a>) -> ValidateInstance<'a> {
        let resolved = project();
        ValidateInstance {
            external_source: None,
            annotated_is_synced: None,
            locked,
            project_id: resolved.project_id,
            project: resolved,
            assigned_pod_id: None,
            agent_executor: None,
        }
    }

    /// Owned id-list filter closure.
    type IdFilter = Box<dyn Fn(&[uuid::Uuid]) -> Vec<uuid::Uuid>>;

    /// Owned probe closures with a borrowing [`ValidateProbes`] view.
    struct ProbeBank {
        git_sync_exists: Box<dyn Fn() -> bool>,
        github_sync_exists: Box<dyn Fn() -> bool>,
        has_active_run: Box<dyn Fn() -> bool>,
        filter_assignees: IdFilter,
        filter_labels: IdFilter,
        state_exists: Box<dyn Fn() -> bool>,
        triage_state_exists: Box<dyn Fn() -> bool>,
        parent_exists: Box<dyn Fn() -> bool>,
        estimate_exists: Box<dyn Fn() -> bool>,
        fetch_project: Box<dyn Fn() -> Option<ResolvedProject>>,
        llm_profile: Box<dyn Fn() -> LlmProfile>,
        enrolled_exists: Box<dyn Fn() -> bool>,
        online_exists: Box<dyn Fn() -> bool>,
    }

    impl ProbeBank {
        /// Everything passes, filters echo, no active run, no sync rows.
        fn permissive() -> Self {
            let resolved = project();
            Self {
                git_sync_exists: Box::new(|| false),
                github_sync_exists: Box::new(|| false),
                has_active_run: Box::new(|| false),
                filter_assignees: Box::new(|ids| ids.to_vec()),
                filter_labels: Box::new(|ids| ids.to_vec()),
                state_exists: Box::new(|| true),
                triage_state_exists: Box::new(|| true),
                parent_exists: Box::new(|| true),
                estimate_exists: Box::new(|| true),
                fetch_project: Box::new(move || Some(resolved)),
                llm_profile: Box::new(|| LlmProfile {
                    available: true,
                    reason_code: String::new(),
                }),
                enrolled_exists: Box::new(|| true),
                online_exists: Box::new(|| true),
            }
        }

        fn probes(&self) -> ValidateProbes<'_> {
            ValidateProbes {
                git_sync_exists: &self.git_sync_exists,
                github_sync_exists: &self.github_sync_exists,
                has_active_run: &self.has_active_run,
                filter_assignees: &self.filter_assignees,
                filter_labels: &self.filter_labels,
                state_exists: &self.state_exists,
                triage_state_exists: &self.triage_state_exists,
                parent_exists: &self.parent_exists,
                estimate_exists: &self.estimate_exists,
                fetch_project: &self.fetch_project,
                llm_profile: &self.llm_profile,
                enrolled_exists: &self.enrolled_exists,
                online_exists: &self.online_exists,
            }
        }
    }

    fn cloud_settings(enabled: bool) -> CloudAgentSettings {
        CloudAgentSettings {
            enabled,
            writes_enabled: false,
            github_tools_enabled: true,
            disabled_tools: Vec::new(),
            reconcile_interval_secs: 30,
            model_request_timeout_secs: 60,
            execution_timeout_secs: 285,
            run_soft_limit_secs: 300,
            run_hard_limit_secs: 330,
            stale_grace_secs: 60,
            dispatch_lease_secs: 60,
            dispatch_backoff_secs: 10,
            dispatch_scan_interval_secs: 10,
            sweep_interval_secs: 30,
            dispatch_scan_batch: 100,
            max_queue_age_secs: 900,
            model_request_limit: 25,
            tool_call_limit: 20,
            write_call_limit: 3,
            input_token_limit: 144_000,
            output_token_limit: 16_000,
            total_token_limit: 160_000,
            max_output_tokens_per_request: 4096,
            max_queued_per_workspace: 20,
            max_running_per_workspace: 2,
            user_creation_rate_per_minute: 6,
            workspace_creation_rate_per_minute: 30,
            tool_timeout_secs: 20,
            max_tool_result_bytes: 65536,
            max_prompt_bytes: 262_144,
            max_final_result_bytes: 65536,
            max_events: 500,
            block_private_urls: true,
        }
    }

    fn managed_settings(enabled: bool) -> ManagedRunnerSettings {
        ManagedRunnerSettings {
            enabled,
            max_per_user_project: 1,
            queued_max_age_secs: 43200,
            graceful_stop_secs: 30,
            sweep_interval_secs: 300,
            desktop_min_version: String::new(),
        }
    }

    fn active_user() -> UserFlags {
        UserFlags {
            is_active: true,
            is_bot: false,
        }
    }

    /// Top-level JSON key order of a struct's serialization, read off the
    /// serialized string: struct serialization always emits declaration
    /// order, while `Value` objects iterate alphabetically.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                }
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    #[test]
    fn complexity_score_replays_fixture() {
        let golden = fixture();
        let message = golden["validate_complexity_score"]["message"]
            .as_str()
            .expect("message str");
        assert_eq!(COMPLEXITY_SCORE_MESSAGE, message);
        assert!(golden["validate_complexity_score"]["none_passthrough"]
            .as_bool()
            .expect("passthrough bool"));
        assert_eq!(validate_complexity_score(None).expect("none passes"), None);
        for score in [0, 5, 10] {
            assert_eq!(
                validate_complexity_score(Some(score)).expect("in range passes"),
                Some(score)
            );
        }
        for score in [-1, 11, 100] {
            let err = validate_complexity_score(Some(score)).expect_err("out of range fails");
            assert_eq!(
                err.raised_detail().expect("raised"),
                serde_json::json!({ "complexity_score": message }),
            );
            assert_eq!(
                err.wire_body().expect("wire"),
                serde_json::json!({ "complexity_score": [message] }),
            );
            assert!(!err.is_server_error());
        }
    }

    /// The 11-step chain (`validate_order`): every step armed to fail at
    /// once, then earlier failures lifted one by one — each step wins
    /// exactly when every step before it passes.
    #[test]
    fn validate_steps_run_in_fixture_order() {
        let golden = fixture();
        let order = golden["validate_order"].as_array().expect("order array");
        assert_eq!(order.len(), 11, "eleven ordered steps");

        let instance_json = serde_json::json!({"a": 1});
        let attrs_json = serde_json::json!({"b": 2});
        let huge = "x".repeat(sticky_kernel::MAX_HTML_BYTES + 1);
        let assignees = [uid(5)];
        let labels = [uid(6)];

        let state_ok = Rc::new(Cell::new(false));
        let parent_ok = Rc::new(Cell::new(false));
        let estimate_ok = Rc::new(Cell::new(false));
        let mut bank = ProbeBank::permissive();
        bank.has_active_run = Box::new(|| true);
        {
            let flag = Rc::clone(&state_ok);
            bank.state_exists = Box::new(move || flag.get());
        }
        {
            let flag = Rc::clone(&parent_ok);
            bank.parent_exists = Box::new(move || flag.get());
        }
        {
            let flag = Rc::clone(&estimate_ok);
            bank.estimate_exists = Box::new(move || flag.get());
        }
        let probes = bank.probes();

        let cloud = cloud_settings(false);
        let managed = managed_settings(false);
        let policy = ExecutorPolicy {
            cloud: &cloud,
            managed: &managed,
            viewer: None,
        };
        let ctx = ValidateContext {
            project_id: None,
            allow_triage_state: false,
        };

        let mut current = instance(locked_values(&instance_json));
        current.external_source = Some("github");
        current.annotated_is_synced = Some(true);

        let mut attrs = base_attrs();
        attrs.locked = LockedIssueAttrs {
            name: Some("New"),
            description_html: Some("<p>new</p>"),
            description_json: Some(&attrs_json),
            description_stripped: Some(Some("new")),
            description_binary_present: false,
        };
        attrs.start_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 1);
        attrs.target_date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1);
        attrs.assigned_pod = Some(Some(PodRef {
            id: uid(10),
            project_id: uid(99),
            deleted: false,
        }));
        attrs.agent_executor = Some(Some("bogus"));
        attrs.description_html = Some(&huge);
        attrs.description_binary = Some("!!!!");
        attrs.assignee_ids = Some(&assignees);
        attrs.label_ids = Some(&labels);
        attrs.state = Some(uid(20));
        attrs.parent = Some(uid(21));
        attrs.estimate_point = Some(uid(22));

        // Step 1 wins over everything.
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("sync lock wins");
        assert!(matches!(err, CreateValidateError::Fields(ref fields) if fields.len() == 4));

        // Step 2 wins once unsynced.
        current.external_source = None;
        current.annotated_is_synced = None;
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("dates win");
        assert_eq!(
            err,
            CreateValidateError::NonField {
                message: DATES_MESSAGE
            }
        );

        // Step 3 wins once dates pass.
        attrs.start_date = None;
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("pod wins");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "assigned_pod_id": POD_DIFFERENT_PROJECT_MESSAGE }),
        );

        // Step 4 wins once the pod key is absent.
        attrs.assigned_pod = None;
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("executor wins");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "agent_executor": EXECUTOR_UNKNOWN_MESSAGE }),
        );

        // Step 5 wins once the executor key is absent.
        attrs.agent_executor = None;
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("html wins");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "error": sticky_kernel::HTML_INVALID_MESSAGE }),
        );

        // Step 6 wins once the html key is absent.
        attrs.description_html = None;
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("binary wins");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "description_binary": sticky_kernel::BINARY_INVALID_MESSAGE }),
        );

        // Step 7 wins once the binary key is absent (missing ctx key = 500).
        attrs.description_binary = None;
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("assignees win");
        assert_eq!(
            err,
            CreateValidateError::ContextMissing { key: "project_id" }
        );
        assert!(err.is_server_error());
        assert_eq!(err.raised_detail(), None);
        assert_eq!(err.wire_body(), None);

        // Step 9 wins once assignees are absent (step 8 passes via echo).
        attrs.assignee_ids = None;
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("state wins");
        assert_eq!(
            err,
            CreateValidateError::NonField {
                message: STATE_INVALID_MESSAGE
            }
        );
        assert_eq!(
            err.wire_body().expect("wire"),
            serde_json::json!({ "non_field_errors": [STATE_INVALID_MESSAGE] }),
        );

        // Steps 10, 11, then success with the label echo applied.
        state_ok.set(true);
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("parent wins");
        assert_eq!(
            err,
            CreateValidateError::NonField {
                message: PARENT_INVALID_MESSAGE
            }
        );
        parent_ok.set(true);
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("estimate wins");
        assert_eq!(
            err,
            CreateValidateError::NonField {
                message: ESTIMATE_INVALID_MESSAGE
            }
        );
        estimate_ok.set(true);
        let validated = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect("all pass");
        assert_eq!(
            validated,
            ValidatedAttrs {
                description_html: None,
                assignee_ids: None,
                label_ids: Some(vec![uid(6)]),
            }
        );
    }

    #[test]
    fn sync_lock_replays_fixture() {
        let golden = fixture();
        let rules = &golden["validate_rules"]["sync_lock"];
        let pinned: Vec<String> = rules["locked_fields"]
            .as_array()
            .expect("locked array")
            .iter()
            .map(|key| key.as_str().expect("key str").to_string())
            .collect();
        assert_eq!(
            LOCKED_ISSUE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            pinned,
        );
        assert_eq!(
            SYNC_LOCKED_MESSAGE,
            rules["message_per_field"].as_str().expect("lock message")
        );

        let bank = ProbeBank::permissive();
        let probes = bank.probes();
        let cloud = cloud_settings(false);
        let managed = managed_settings(false);
        let policy = ExecutorPolicy {
            cloud: &cloud,
            managed: &managed,
            viewer: None,
        };
        let ctx = ValidateContext {
            project_id: Some(uid(1)),
            allow_triage_state: false,
        };
        let instance_json = serde_json::json!({"a": 1});

        let mut current = instance(locked_values(&instance_json));
        current.external_source = Some("github");
        current.annotated_is_synced = Some(true);

        // Unchanged re-PATCH passes the lock.
        let mut attrs = base_attrs();
        attrs.locked = LockedIssueAttrs {
            name: Some("Old"),
            description_html: Some("<p>old</p>"),
            description_json: Some(&instance_json),
            description_stripped: Some(Some("old")),
            description_binary_present: false,
        };
        issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect("unchanged passes");

        // Changed fields block in LOCKED_ISSUE_FIELDS order.
        let other_json = serde_json::json!({"b": 2});
        attrs.locked = LockedIssueAttrs {
            name: Some("New"),
            description_html: Some("<p>old</p>"),
            description_json: Some(&other_json),
            description_stripped: Some(None),
            description_binary_present: true,
        };
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("changed fields block");
        let CreateValidateError::Fields(fields) = &err else {
            panic!("expected multi-field lock, got {err:?}");
        };
        let names: Vec<&str> = fields.iter().map(|(field, _)| *field).collect();
        assert_eq!(
            names,
            [
                "name",
                "description_json",
                "description_stripped",
                "description_binary"
            ]
        );
        for (_, message) in fields {
            assert_eq!(message.as_ref(), SYNC_LOCKED_MESSAGE);
        }
        let wire = err.wire_body().expect("wire");
        assert_eq!(
            wire["name"],
            serde_json::json!([SYNC_LOCKED_MESSAGE]),
            "single-field body is byte-exact"
        );

        // Create path (no instance) skips the lock entirely.
        issue_create_validate(&attrs, &ctx, None, &policy, &probes).expect("create skips lock");

        // Unsynced instance skips the lock; the git probe short-circuits.
        current.annotated_is_synced = None;
        let git_calls = Rc::new(Cell::new(0u32));
        let github_calls = Rc::new(Cell::new(0u32));
        let mut counting = ProbeBank::permissive();
        {
            let calls = Rc::clone(&git_calls);
            counting.git_sync_exists = Box::new(move || {
                calls.set(calls.get() + 1);
                true
            });
        }
        {
            let calls = Rc::clone(&github_calls);
            counting.github_sync_exists = Box::new(move || {
                calls.set(calls.get() + 1);
                true
            });
        }
        let counting_probes = counting.probes();
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &counting_probes)
            .expect_err("git probe hit locks");
        assert!(matches!(err, CreateValidateError::Fields(_)));
        assert_eq!(git_calls.get(), 1);
        assert_eq!(github_calls.get(), 0, "git hit short-circuits github");
    }

    #[test]
    fn pod_arms_replay_fixture() {
        let golden = fixture();
        let rules = &golden["validate_rules"]["assigned_pod"];
        assert_eq!(
            POD_DIFFERENT_PROJECT_MESSAGE,
            rules["different_project"]["message"]["assigned_pod_id"]
                .as_str()
                .expect("pod project message")
        );
        assert_eq!(
            POD_DELETED_MESSAGE,
            rules["deleted"]["message"]["assigned_pod_id"]
                .as_str()
                .expect("pod deleted message")
        );
        assert_eq!(
            POD_REASSIGN_MESSAGE,
            rules["reassign_mid_flight"]["message"]["assigned_pod_id"]
                .as_str()
                .expect("pod reassign message")
        );

        let bank = ProbeBank::permissive();
        let probes = bank.probes();
        let cloud = cloud_settings(false);
        let managed = managed_settings(false);
        let policy = ExecutorPolicy {
            cloud: &cloud,
            managed: &managed,
            viewer: None,
        };
        let instance_json = serde_json::json!({});
        let current = instance(locked_values(&instance_json));

        // Different project via the context leg.
        let ctx = ValidateContext {
            project_id: Some(uid(1)),
            allow_triage_state: false,
        };
        let mut attrs = base_attrs();
        attrs.assigned_pod = Some(Some(PodRef {
            id: uid(10),
            project_id: uid(99),
            deleted: false,
        }));
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("different project fails");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "assigned_pod_id": POD_DIFFERENT_PROJECT_MESSAGE }),
        );

        // The attrs-project leg (create: no ctx project, no instance).
        attrs.attrs_project = Some(project());
        let err = issue_create_validate(&attrs, &ctx, None, &policy, &probes)
            .expect_err("ctx still wins on create");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "assigned_pod_id": POD_DIFFERENT_PROJECT_MESSAGE }),
        );
        let no_ctx = ValidateContext {
            project_id: None,
            allow_triage_state: false,
        };
        let err = issue_create_validate(&attrs, &no_ctx, None, &policy, &probes)
            .expect_err("attrs leg engages");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "assigned_pod_id": POD_DIFFERENT_PROJECT_MESSAGE }),
        );

        // Same project passes; deleted fails.
        attrs.assigned_pod = Some(Some(PodRef {
            id: uid(10),
            project_id: uid(1),
            deleted: true,
        }));
        attrs.attrs_project = None;
        let err = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect_err("deleted fails");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "assigned_pod_id": POD_DELETED_MESSAGE }),
        );

        // Null clears: project/deleted checks skipped, mid-flight still runs.
        let mut busy = ProbeBank::permissive();
        busy.has_active_run = Box::new(|| true);
        let busy_probes = busy.probes();
        let mut assigned = instance(locked_values(&instance_json));
        assigned.assigned_pod_id = Some(uid(10));
        let mut clear = base_attrs();
        clear.assigned_pod = Some(None);
        let err = issue_create_validate(&clear, &ctx, Some(&assigned), &policy, &busy_probes)
            .expect_err("clear counts as a change");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "assigned_pod_id": POD_REASSIGN_MESSAGE }),
        );
        // Initial assignment is always allowed, even mid-run.
        let mut fresh = instance(locked_values(&instance_json));
        fresh.assigned_pod_id = None;
        let mut assign = base_attrs();
        assign.assigned_pod = Some(Some(PodRef {
            id: uid(10),
            project_id: uid(1),
            deleted: false,
        }));
        issue_create_validate(&assign, &ctx, Some(&fresh), &policy, &busy_probes)
            .expect("initial assignment allowed");
        // Same pod is not a change.
        let mut same = base_attrs();
        same.assigned_pod = Some(Some(PodRef {
            id: uid(10),
            project_id: uid(1),
            deleted: false,
        }));
        issue_create_validate(&same, &ctx, Some(&assigned), &policy, &busy_probes)
            .expect("same pod skips mid-flight");
    }

    #[test]
    fn executor_branch_replays_fixture() {
        let golden = fixture();
        let rules = &golden["validate_rules"]["agent_executor"];
        let valid: Vec<String> = rules["unknown_executor"]["valid_values"]
            .as_array()
            .expect("valid array")
            .iter()
            .map(|value| value.as_str().expect("value str").to_string())
            .collect();
        for value in &valid {
            assert!(
                AgentExecutorKind::from_value(value).is_some(),
                "fixture value parses: {value}"
            );
        }
        assert_eq!(
            EXECUTOR_UNKNOWN_MESSAGE,
            rules["unknown_executor"]["message"]["agent_executor"]
                .as_str()
                .expect("unknown message")
        );
        assert_eq!(
            EXECUTOR_CLOUD_UNAVAILABLE_MESSAGE,
            rules["cloud_agent"]["message"]["agent_executor"]
                .as_str()
                .expect("cloud message")
        );
        assert_eq!(
            EXECUTOR_PROJECT_REQUIRED_MESSAGE,
            rules["managed_runner"]["missing_project_message"]["agent_executor"]
                .as_str()
                .expect("project message")
        );
        assert_eq!(
            EXECUTOR_MID_FLIGHT_MESSAGE,
            rules["mid_flight"]["message"]["agent_executor"]
                .as_str()
                .expect("mid-flight message")
        );
        let copy = &rules["reason_copy"];
        assert_eq!(
            managed_unavailable_detail(ManagedRunnerReason::DISABLED).as_ref(),
            copy["managed_runner_disabled"]
                .as_str()
                .expect("disabled copy")
        );
        assert_eq!(
            managed_unavailable_detail(ManagedRunnerReason::NOT_CONNECTED).as_ref(),
            copy["desktop_not_connected"]
                .as_str()
                .expect("offline copy")
        );
        assert_eq!(
            managed_unavailable_detail(ManagedRunnerReason::LLM_CONFIG_MISSING).as_ref(),
            copy["llm_config_missing"].as_str().expect("llm copy")
        );
        assert_eq!(
            managed_unavailable_detail(ManagedRunnerReason::GATEWAY_SCOPES_MISSING).as_ref(),
            copy["gateway_scopes_missing"]
                .as_str()
                .expect("scopes copy")
        );
        assert_eq!(
            managed_unavailable_detail(ManagedRunnerReason::BYOK_UNSUPPORTED).as_ref(),
            copy["byok_not_supported_on_desktop"]
                .as_str()
                .expect("byok copy")
        );
        assert_eq!(
            managed_unavailable_detail("something_new").as_ref(),
            "something_new",
            "unknown reason falls back to the raw code"
        );

        let ctx = ValidateContext {
            project_id: Some(uid(1)),
            allow_triage_state: false,
        };
        let instance_json = serde_json::json!({});

        // Unknown executor (fires only for values past the ChoiceField).
        let bank = ProbeBank::permissive();
        let probes = bank.probes();
        let cloud = cloud_settings(false);
        let managed = managed_settings(false);
        let policy = ExecutorPolicy {
            cloud: &cloud,
            managed: &managed,
            viewer: None,
        };
        let mut attrs = base_attrs();
        attrs.agent_executor = Some(Some("bogus"));
        let err = issue_create_validate(&attrs, &ctx, None, &policy, &probes)
            .expect_err("unknown executor fails");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "agent_executor": EXECUTOR_UNKNOWN_MESSAGE }),
        );

        // Cloud arm follows the instance switch.
        attrs.agent_executor = Some(Some("cloud_agent"));
        let err = issue_create_validate(&attrs, &ctx, None, &policy, &probes)
            .expect_err("cloud disabled fails");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "agent_executor": EXECUTOR_CLOUD_UNAVAILABLE_MESSAGE }),
        );
        let cloud_on = cloud_settings(true);
        let policy_on = ExecutorPolicy {
            cloud: &cloud_on,
            managed: &managed,
            viewer: None,
        };
        issue_create_validate(&attrs, &ctx, None, &policy_on, &probes)
            .expect("cloud enabled passes");

        // Managed arm without any resolvable project.
        let mut no_project = ProbeBank::permissive();
        no_project.fetch_project = Box::new(|| None);
        let no_project_probes = no_project.probes();
        attrs.agent_executor = Some(Some("managed_runner"));
        let err = issue_create_validate(&attrs, &ctx, None, &policy_on, &no_project_probes)
            .expect_err("missing project fails");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "agent_executor": EXECUTOR_PROJECT_REQUIRED_MESSAGE }),
        );

        // Managed arm refused with the reason copy (instance switch off).
        let mut current = instance(locked_values(&instance_json));
        current.agent_executor = Some("managed_runner");
        let err =
            issue_create_validate(&attrs, &ctx, Some(&current), &policy_on, &no_project_probes)
                .expect_err("managed disabled fails");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "agent_executor": copy["managed_runner_disabled"] }),
        );

        // no_managed_runner_for_project self-heals: accepted.
        let managed_on = managed_settings(true);
        let viewer = active_user();
        let policy_managed = ExecutorPolicy {
            cloud: &cloud_on,
            managed: &managed_on,
            viewer: Some(&viewer),
        };
        let mut unenrolled = ProbeBank::permissive();
        unenrolled.enrolled_exists = Box::new(|| false);
        let unenrolled_probes = unenrolled.probes();
        issue_create_validate(
            &attrs,
            &ctx,
            Some(&current),
            &policy_managed,
            &unenrolled_probes,
        )
        .expect("no-runner-for-project passes");

        // Every other failing reason refuses with its copy.
        for reason in [
            ManagedRunnerReason::LLM_CONFIG_MISSING,
            ManagedRunnerReason::GATEWAY_SCOPES_MISSING,
            ManagedRunnerReason::BYOK_UNSUPPORTED,
        ] {
            let owned = reason.to_owned();
            let mut failing = ProbeBank::permissive();
            failing.llm_profile = Box::new(move || LlmProfile {
                available: false,
                reason_code: owned.clone(),
            });
            let failing_probes = failing.probes();
            let err = issue_create_validate(
                &attrs,
                &ctx,
                Some(&current),
                &policy_managed,
                &failing_probes,
            )
            .unwrap_err();
            assert_eq!(
                err.raised_detail().expect("raised"),
                serde_json::json!({ "agent_executor": managed_unavailable_detail(reason).as_ref() }),
                "reason {reason} refuses with its copy"
            );
        }

        // local_runner needs no policy at all.
        attrs.agent_executor = Some(Some("local_runner"));
        issue_create_validate(&attrs, &ctx, None, &policy, &probes).expect("local passes");

        // Mid-flight change is refused; clearing to the same value passes.
        let mut busy = ProbeBank::permissive();
        busy.has_active_run = Box::new(|| true);
        let busy_probes = busy.probes();
        let mut flying = instance(locked_values(&instance_json));
        flying.agent_executor = Some("local_runner");
        attrs.agent_executor = Some(Some("cloud_agent"));
        let err = issue_create_validate(&attrs, &ctx, Some(&flying), &policy_on, &busy_probes)
            .expect_err("mid-flight change fails");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "agent_executor": EXECUTOR_MID_FLIGHT_MESSAGE }),
        );
        flying.agent_executor = Some("managed_runner");
        attrs.agent_executor = Some(Some("managed_runner"));
        issue_create_validate(&attrs, &ctx, Some(&flying), &policy_managed, &busy_probes)
            .expect("unchanged executor skips mid-flight");
    }

    #[test]
    fn sanitize_arms_replay_fixture() {
        let golden = fixture();
        assert_eq!(
            sticky_kernel::HTML_INVALID_MESSAGE,
            golden["validate_rules"]["description_html"]["invalid_message"]["error"]
                .as_str()
                .expect("html message")
        );
        assert_eq!(
            sticky_kernel::BINARY_INVALID_MESSAGE,
            golden["validate_rules"]["description_binary"]["invalid_message"]["description_binary"]
                .as_str()
                .expect("binary message")
        );

        let bank = ProbeBank::permissive();
        let probes = bank.probes();
        let cloud = cloud_settings(false);
        let managed = managed_settings(false);
        let policy = ExecutorPolicy {
            cloud: &cloud,
            managed: &managed,
            viewer: None,
        };
        let ctx = ValidateContext {
            project_id: Some(uid(1)),
            allow_triage_state: false,
        };

        // Oversize HTML is invalid; the field is the literal "error".
        let huge = "x".repeat(sticky_kernel::MAX_HTML_BYTES + 1);
        let mut attrs = base_attrs();
        attrs.description_html = Some(&huge);
        let err = issue_create_validate(&attrs, &ctx, None, &policy, &probes)
            .expect_err("oversize html fails");
        assert_eq!(
            err.wire_body().expect("wire"),
            serde_json::json!({ "error": [sticky_kernel::HTML_INVALID_MESSAGE] }),
        );

        // Valid HTML is replaced by the cleaned output; empty input is kept.
        attrs.description_html = Some("<p>hi</p>");
        let validated =
            issue_create_validate(&attrs, &ctx, None, &policy, &probes).expect("clean html passes");
        assert_eq!(validated.description_html.as_deref(), Some("<p>hi</p>"));
        attrs.description_html = Some("<p>hi</p><script>alert(1)</script>");
        let validated =
            issue_create_validate(&attrs, &ctx, None, &policy, &probes).expect("script stripped");
        assert_eq!(validated.description_html.as_deref(), Some("<p>hi</p>"));
        attrs.description_html = Some("");
        let validated =
            issue_create_validate(&attrs, &ctx, None, &policy, &probes).expect("empty kept");
        assert_eq!(validated.description_html, None);

        // Invalid binary reports its literal message.
        attrs.description_html = None;
        attrs.description_binary = Some("!!!!");
        let err = issue_create_validate(&attrs, &ctx, None, &policy, &probes)
            .expect_err("bad binary fails");
        assert_eq!(
            err.raised_detail().expect("raised"),
            serde_json::json!({ "description_binary": sticky_kernel::BINARY_INVALID_MESSAGE }),
        );
    }

    #[test]
    fn assignee_label_filters_silently_drop() {
        let bank = ProbeBank::permissive();
        let probes = bank.probes();
        let cloud = cloud_settings(false);
        let managed = managed_settings(false);
        let policy = ExecutorPolicy {
            cloud: &cloud,
            managed: &managed,
            viewer: None,
        };
        let ctx = ValidateContext {
            project_id: Some(uid(1)),
            allow_triage_state: false,
        };

        // The surviving subset (in probe order) replaces the input.
        let mut dropping = ProbeBank::permissive();
        dropping.filter_assignees = Box::new(|_| vec![uid(7), uid(5)]);
        dropping.filter_labels = Box::new(|_| Vec::new());
        let dropping_probes = dropping.probes();
        let ids = [uid(5), uid(6), uid(7)];
        let mut attrs = base_attrs();
        attrs.assignee_ids = Some(&ids);
        attrs.label_ids = Some(&ids);
        let validated =
            issue_create_validate(&attrs, &ctx, None, &policy, &dropping_probes).expect("drop ok");
        assert_eq!(validated.assignee_ids, Some(vec![uid(7), uid(5)]));
        assert_eq!(validated.label_ids, Some(Vec::new()));

        // Empty input skips the probe entirely (no mutation recorded).
        let calls = Rc::new(Cell::new(0u32));
        let mut counting = ProbeBank::permissive();
        {
            let flag = Rc::clone(&calls);
            counting.filter_assignees = Box::new(move |ids| {
                flag.set(flag.get() + 1);
                ids.to_vec()
            });
        }
        let counting_probes = counting.probes();
        let mut empty = base_attrs();
        let none: [uuid::Uuid; 0] = [];
        empty.assignee_ids = Some(&none);
        let validated =
            issue_create_validate(&empty, &ctx, None, &policy, &counting_probes).expect("empty ok");
        assert_eq!(validated.assignee_ids, None);
        assert_eq!(calls.get(), 0);

        // Assignees use context[...] (KeyError parity); labels use .get.
        let no_ctx = ValidateContext {
            project_id: None,
            allow_triage_state: false,
        };
        let mut missing = base_attrs();
        missing.assignee_ids = Some(&ids);
        let err = issue_create_validate(&missing, &no_ctx, None, &policy, &probes)
            .expect_err("missing ctx project is a 500");
        assert_eq!(
            err,
            CreateValidateError::ContextMissing { key: "project_id" }
        );
        let mut labels_only = base_attrs();
        labels_only.label_ids = Some(&ids);
        let validated = issue_create_validate(&labels_only, &no_ctx, None, &policy, &probes)
            .expect("labels tolerate a None project");
        assert_eq!(validated.label_ids, Some(vec![uid(5), uid(6), uid(7)]));
    }

    #[test]
    fn state_parent_estimate_replay_fixture() {
        let golden = fixture();
        assert_eq!(
            STATE_INVALID_MESSAGE,
            golden["validate_rules"]["state_check"]["message"]
                .as_str()
                .expect("state message")
        );
        assert_eq!(
            PARENT_INVALID_MESSAGE,
            golden["validate_rules"]["parent_check"]["message"]
                .as_str()
                .expect("parent message")
        );
        assert_eq!(
            ESTIMATE_INVALID_MESSAGE,
            golden["validate_rules"]["estimate_check"]["message"]
                .as_str()
                .expect("estimate message")
        );

        let cloud = cloud_settings(false);
        let managed = managed_settings(false);
        let policy = ExecutorPolicy {
            cloud: &cloud,
            managed: &managed,
            viewer: None,
        };

        // allow_triage_state switches the manager (probe), not the message.
        let state_calls = Rc::new(Cell::new(0u32));
        let triage_calls = Rc::new(Cell::new(0u32));
        let mut bank = ProbeBank::permissive();
        {
            let flag = Rc::clone(&state_calls);
            bank.state_exists = Box::new(move || {
                flag.set(flag.get() + 1);
                false
            });
        }
        {
            let flag = Rc::clone(&triage_calls);
            bank.triage_state_exists = Box::new(move || {
                flag.set(flag.get() + 1);
                true
            });
        }
        let probes = bank.probes();
        let mut attrs = base_attrs();
        attrs.state = Some(uid(20));
        let strict = ValidateContext {
            project_id: Some(uid(1)),
            allow_triage_state: false,
        };
        let err = issue_create_validate(&attrs, &strict, None, &policy, &probes)
            .expect_err("default manager misses");
        assert_eq!(
            err,
            CreateValidateError::NonField {
                message: STATE_INVALID_MESSAGE
            }
        );
        assert_eq!((state_calls.get(), triage_calls.get()), (1, 0));
        let intake = ValidateContext {
            project_id: Some(uid(1)),
            allow_triage_state: true,
        };
        issue_create_validate(&attrs, &intake, None, &policy, &probes).expect("triage admits");
        assert_eq!((state_calls.get(), triage_calls.get()), (1, 1));

        // Parent and estimate are plain exists checks with non-field errors.
        let mut missing = ProbeBank::permissive();
        missing.parent_exists = Box::new(|| false);
        missing.estimate_exists = Box::new(|| false);
        let missing_probes = missing.probes();
        let mut kin = base_attrs();
        kin.parent = Some(uid(21));
        kin.estimate_point = Some(uid(22));
        let err = issue_create_validate(&kin, &strict, None, &policy, &missing_probes)
            .expect_err("parent misses");
        assert_eq!(
            err,
            CreateValidateError::NonField {
                message: PARENT_INVALID_MESSAGE
            }
        );
        kin.parent = None;
        let err = issue_create_validate(&kin, &strict, None, &policy, &missing_probes)
            .expect_err("estimate misses");
        assert_eq!(
            err,
            CreateValidateError::NonField {
                message: ESTIMATE_INVALID_MESSAGE
            }
        );
    }

    #[test]
    fn absent_keys_run_no_probes() {
        let counters: Vec<Rc<Cell<u32>>> = (0..12).map(|_| Rc::new(Cell::new(0))).collect();
        let bump = |index: usize| {
            let counter = Rc::clone(&counters[index]);
            move || {
                counter.set(counter.get() + 1);
            }
        };
        // Each closure owns its counter; probes must stay silent on empty attrs.
        let git = bump(0);
        let github = bump(1);
        let active = bump(2);
        let assignees = bump(3);
        let labels = bump(4);
        let state = bump(5);
        let triage = bump(6);
        let parent = bump(7);
        let estimate = bump(8);
        let fetch = bump(9);
        let llm = bump(10);
        let enrolled_online = Rc::new(Cell::new(0u32));
        let bank = ProbeBank {
            git_sync_exists: Box::new(move || {
                git();
                false
            }),
            github_sync_exists: Box::new(move || {
                github();
                false
            }),
            has_active_run: Box::new(move || {
                active();
                false
            }),
            filter_assignees: Box::new(move |ids| {
                assignees();
                ids.to_vec()
            }),
            filter_labels: Box::new(move |ids| {
                labels();
                ids.to_vec()
            }),
            state_exists: Box::new(move || {
                state();
                true
            }),
            triage_state_exists: Box::new(move || {
                triage();
                true
            }),
            parent_exists: Box::new(move || {
                parent();
                true
            }),
            estimate_exists: Box::new(move || {
                estimate();
                true
            }),
            fetch_project: Box::new(move || {
                fetch();
                Some(project())
            }),
            llm_profile: Box::new(move || {
                llm();
                LlmProfile {
                    available: true,
                    reason_code: String::new(),
                }
            }),
            enrolled_exists: {
                let counter = Rc::clone(&enrolled_online);
                Box::new(move || {
                    counter.set(counter.get() + 1);
                    true
                })
            },
            online_exists: {
                let counter = Rc::clone(&enrolled_online);
                Box::new(move || {
                    counter.set(counter.get() + 1);
                    true
                })
            },
        };
        let probes = bank.probes();
        let cloud = cloud_settings(false);
        let managed = managed_settings(false);
        let policy = ExecutorPolicy {
            cloud: &cloud,
            managed: &managed,
            viewer: None,
        };
        let ctx = ValidateContext {
            project_id: Some(uid(1)),
            allow_triage_state: false,
        };
        let instance_json = serde_json::json!({});
        let current = instance(locked_values(&instance_json));
        let attrs = base_attrs();
        let validated = issue_create_validate(&attrs, &ctx, Some(&current), &policy, &probes)
            .expect("empty attrs pass");
        assert_eq!(
            validated,
            ValidatedAttrs {
                description_html: None,
                assignee_ids: None,
                label_ids: None,
            }
        );
        for (index, counter) in counters.iter().enumerate() {
            assert_eq!(counter.get(), 0, "probe {index} silent on empty attrs");
        }
        assert_eq!(enrolled_online.get(), 0, "runner probes silent");
    }

    #[test]
    fn representation_echo_replays_fixture() {
        // The echo arm: present-and-truthy passes through, else [].
        assert_eq!(echo_initial_ids(None), serde_json::json!([]));
        for falsy in [
            serde_json::json!(null),
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!(""),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            assert_eq!(echo_initial_ids(Some(&falsy)), serde_json::json!([]));
        }
        let sent = serde_json::json!(["a", "b"]);
        assert_eq!(echo_initial_ids(Some(&sent)), sent);

        // 41 wire keys with the echo pair appended last (:187-190).
        assert_eq!(ISSUE_CREATE_FIELDS.len(), 41);
        assert_eq!(ISSUE_CREATE_FIELDS[0], "id");
        assert_eq!(
            &ISSUE_CREATE_FIELDS[ISSUE_CREATE_FIELDS.len() - 2..],
            &["assignee_ids", "label_ids"]
        );

        let description_json = serde_json::json!({"ops": []});
        let binary = b"<p>hi</p>";
        let assignees: [&str; 1] = ["aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"];
        let labels: [&str; 0] = [];
        let row = IssueCreateRow {
            id: "11111111-1111-1111-1111-111111111111",
            state_id: Some("22222222-2222-2222-2222-222222222222"),
            parent_id: None,
            assigned_pod_id: None,
            project_id: "33333333-3333-3333-3333-333333333333",
            workspace_id: "44444444-4444-4444-4444-444444444444",
            created_at: "2026-10-03T00:00:00Z",
            updated_at: "2026-10-03T00:00:01Z",
            deleted_at: None,
            point: Some(3),
            name: "Ship it",
            description_json: &description_json,
            description_html: "<p>hi</p>",
            description_stripped: Some("hi"),
            description_binary: Some(binary),
            priority: "high",
            complexity_score: 3,
            start_date: Some("2026-10-01"),
            target_date: None,
            sequence_id: 7,
            sort_order: 1.5,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            git_work_branch: "",
            created_via: None,
            agent_executor: Some("local_runner"),
            created_by: Some("55555555-5555-5555-5555-555555555555"),
            updated_by: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "44444444-4444-4444-4444-444444444444",
            parent: None,
            state: Some("22222222-2222-2222-2222-222222222222"),
            estimate_point: None,
            issue_type: None,
            assignees: &assignees,
            labels: &labels,
        };
        let view = issue_create_to_representation(&row, None, Some(&sent));
        assert_eq!(
            serialized_keys(&view),
            ISSUE_CREATE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(view.assignee_ids, serde_json::json!([]));
        assert_eq!(
            view.label_ids, sent,
            "echo follows the request, not the rows"
        );
        assert_eq!(view.assignees, vec![assignees[0].to_string()]);
        assert!(view.labels.is_empty());
        assert_eq!(
            view.description_binary.as_deref(),
            Some("PHA+aGk8L3A+"),
            "BinaryField.value_to_string is base64"
        );
        assert_eq!(view.issue_type, None);
        let rendered = serde_json::to_value(&view).expect("value");
        assert!(rendered.get("type").is_some(), "`type` renames on the wire");
    }

    #[test]
    fn m2m_write_specs() {
        // bulk_create(batch_size=10) chunking, input order.
        assert!(m2m_batches(&[]).is_empty());
        let ids: Vec<uuid::Uuid> = (0..25).map(uid).collect();
        let batches = m2m_batches(&ids);
        assert_eq!(
            batches.iter().map(|batch| batch.len()).collect::<Vec<_>>(),
            [10, 10, 5]
        );
        assert_eq!(batches.concat(), ids, "order preserved");
        let ten: Vec<uuid::Uuid> = (0..10).map(uid).collect();
        assert_eq!(m2m_batches(&ten).len(), 1);
        let eleven: Vec<uuid::Uuid> = (0..11).map(uid).collect();
        assert_eq!(
            m2m_batches(&eleven)
                .iter()
                .map(|batch| batch.len())
                .collect::<Vec<_>>(),
            [10, 1]
        );

        // One multi-row INSERT per chunk: 9 params/row, NULL deleted_at.
        let sql = m2m_insert_sql("issue_assignees", "assignee_id", 2, false);
        assert!(sql.starts_with("INSERT INTO issue_assignees ("), "{sql}");
        assert!(sql.contains(M2M_INSERT_COLUMNS), "{sql}");
        assert!(sql.contains("assignee_id"), "{sql}");
        assert!(
            sql.contains(", NULL,"),
            "deleted_at is a literal NULL: {sql}"
        );
        assert!(
            !sql.contains("ON CONFLICT"),
            "create has no ignore_conflicts: {sql}"
        );
        for placeholder in 1..=18 {
            assert!(
                sql.contains(&format!("${placeholder}")),
                "missing ${placeholder}: {sql}"
            );
        }
        let update_sql = m2m_insert_sql("issue_labels", "label_id", 1, true);
        assert!(
            update_sql.ends_with(" ON CONFLICT DO NOTHING"),
            "update ignore_conflicts: {update_sql}"
        );

        // Default-assignee fallback truth table (:420-441).
        assert_eq!(
            create_default_assignee_fallback(None, Some(uid(9)), true),
            Some(uid(9))
        );
        assert_eq!(
            create_default_assignee_fallback(Some(&[]), Some(uid(9)), true),
            Some(uid(9))
        );
        assert_eq!(
            create_default_assignee_fallback(Some(&[uid(5)]), Some(uid(9)), true),
            None,
            "non-empty list wins"
        );
        assert_eq!(
            create_default_assignee_fallback(None, None, true),
            None,
            "no default configured"
        );
        assert_eq!(
            create_default_assignee_fallback(None, Some(uid(9)), false),
            None,
            "default is not an active member"
        );

        // Context keys create() requires with [] (:391-393).
        let golden = fixture();
        let pinned: Vec<String> = golden["create_writes"]["context_keys_required"]
            .as_array()
            .expect("keys array")
            .iter()
            .map(|key| key.as_str().expect("key str").to_string())
            .collect();
        assert_eq!(
            REQUIRED_CREATE_CONTEXT_KEYS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            pinned,
        );
    }

    #[test]
    fn sql_consts_pin_tables_and_guards() {
        // Pod.all_objects: tombstones resolve (no soft-delete filter).
        assert!(POD_FETCH_SQL.contains("FROM pod"), "{POD_FETCH_SQL}");
        assert!(
            !POD_FETCH_SQL.contains("deleted_at IS NULL"),
            "{POD_FETCH_SQL}"
        );
        // State managers: triage exclusion vs triage-only.
        assert!(
            STATE_EXISTS_SQL.contains("NOT (\"group\" = 'triage')"),
            "{STATE_EXISTS_SQL}"
        );
        assert!(
            STATE_TRIAGE_EXISTS_SQL.contains("\"group\" = 'triage'"),
            "{STATE_TRIAGE_EXISTS_SQL}"
        );
        assert!(
            !STATE_TRIAGE_EXISTS_SQL.contains("NOT (\"group\""),
            "{STATE_TRIAGE_EXISTS_SQL}"
        );
        // Default soft-delete guards + model ordering on .first().
        for sql in [PARENT_EXISTS_SQL, ESTIMATE_EXISTS_SQL, PROJECT_FETCH_SQL] {
            assert!(sql.contains("deleted_at IS NULL"), "{sql}");
        }
        assert!(
            PROJECT_FETCH_SQL.contains("ORDER BY created_at DESC LIMIT 1"),
            "{PROJECT_FETCH_SQL}"
        );
        // has_active_run: plain manager, lowercase statuses in tuple order.
        assert!(
            !HAS_ACTIVE_RUN_SQL.contains("deleted_at"),
            "{HAS_ACTIVE_RUN_SQL}"
        );
        let golden = fixture();
        let statuses = golden["validate_rules"]["has_active_run"]["statuses"]
            .as_array()
            .expect("statuses array");
        assert_eq!(statuses.len(), 8);
        let mut cursor = 0;
        for status in statuses {
            let needle = format!("'{}'", status.as_str().expect("status str").to_lowercase());
            let rest = &HAS_ACTIVE_RUN_SQL[cursor..];
            let offset = rest.find(needle.as_str()).expect("status present");
            cursor += offset + needle.len();
        }
        // Default-assignee membership gate (:423-430).
        assert!(
            DEFAULT_ASSIGNEE_EXISTS_SQL.contains("role >= 15"),
            "{DEFAULT_ASSIGNEE_EXISTS_SQL}"
        );
        assert!(
            DEFAULT_ASSIGNEE_EXISTS_SQL.contains("is_active"),
            "{DEFAULT_ASSIGNEE_EXISTS_SQL}"
        );
        // Update clears are soft deletes scoped to live rows.
        for sql in [ASSIGNEE_CLEAR_SQL, LABEL_CLEAR_SQL] {
            assert!(sql.starts_with("UPDATE "), "{sql}");
            assert!(sql.contains("SET deleted_at = $1"), "{sql}");
            assert!(sql.contains("deleted_at IS NULL"), "{sql}");
            assert!(!sql.contains("updated_at"), "auto_now untouched: {sql}");
        }
        assert!(
            ASSIGNEE_CLEAR_SQL.contains("issue_assignees"),
            "{ASSIGNEE_CLEAR_SQL}"
        );
        assert!(
            LABEL_CLEAR_SQL.contains("issue_labels"),
            "{LABEL_CLEAR_SQL}"
        );
        // Assignee/label filters: guards, order, IN-list forms.
        let member_sql = assignee_member_filter_sql(2);
        assert!(member_sql.contains("role >= 15"), "{member_sql}");
        assert!(
            member_sql.contains("ORDER BY created_at DESC"),
            "{member_sql}"
        );
        assert!(member_sql.contains("member_id IN ($2, $3)"), "{member_sql}");
        let label_sql = label_filter_sql(1);
        assert!(label_sql.contains("project_id = $1"), "{label_sql}");
        assert!(label_sql.contains("id IN ($2)"), "{label_sql}");
        let null_sql = label_filter_null_project_sql(2);
        assert!(null_sql.contains("project_id IS NULL"), "{null_sql}");
        assert!(null_sql.contains("id IN ($1, $2)"), "{null_sql}");
    }

    #[test]
    fn raised_vs_wire_envelopes() {
        // Field errors: bare-string dict raised, list-wrapped on the wire.
        let field = CreateValidateError::Field {
            field: "agent_executor",
            message: Cow::Borrowed(EXECUTOR_UNKNOWN_MESSAGE),
        };
        assert_eq!(
            field.raised_detail().expect("raised"),
            serde_json::json!({ "agent_executor": EXECUTOR_UNKNOWN_MESSAGE }),
        );
        assert_eq!(
            field.wire_body().expect("wire"),
            serde_json::json!({ "agent_executor": [EXECUTOR_UNKNOWN_MESSAGE] }),
        );
        assert!(!field.is_server_error());

        // Non-field errors: bare string raised, non_field_errors on the wire.
        let dates = CreateValidateError::NonField {
            message: DATES_MESSAGE,
        };
        assert_eq!(
            dates.raised_detail().expect("raised"),
            serde_json::json!(DATES_MESSAGE),
        );
        assert_eq!(
            dates.wire_body().expect("wire"),
            serde_json::json!({ "non_field_errors": [DATES_MESSAGE] }),
        );
    }
}
