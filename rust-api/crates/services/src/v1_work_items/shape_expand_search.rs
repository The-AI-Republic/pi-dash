#![forbid(unsafe_code)]

//! Expand + issue-link + search shapes (D-18 serializers F, PIDASHCONV-665).
//!
//! Ports 4 units from `apps/api/pi_dash/api/serializers/issue.py`:
//!
//! * `IssueExpandSerializer` (`:1074-1118`, incl. `get_labels` `:1090-1096`
//!   and `get_assignees` `:1097-1118`) → [`render_expand`].
//! * `CycleIssueSerializer` (`:1033-1046`) → [`render_cycle_issue`].
//! * `ModuleIssueSerializer` (`:1047-1060`) → [`render_module_issue`].
//! * Search family (`:1139-1203`: `IssueSearchSerializer`,
//!   `IssueAdvancedSearchState/Project/Result/ResponseSerializer` — one
//!   closure) → [`render_issue_search`], [`validate_issue_search`],
//!   [`render_advanced_result`], [`render_advanced_response`].
//!
//! Fixture: the F18-03 expand/search subset
//! (`rust-api/fixtures/v1_work_items/serializers/`
//! `F18-03.comment_attachment_activity_expand_search.golden.json`).
//! Every `#[test]` below replays it: golden in/out byte-identical,
//! including key orders and the validation error strings/codes.
//!
//! This module is pure: datetimes/dates cross this boundary already rendered
//! as DRF strings (the handler owns timezone conversion and DRF ISO-8601,
//! following the [`shape_issue`](super::shape_issue) precedent) — rendering
//! here is a byte-exact passthrough. Queryset order (`-created_at`) and the
//! `?fields=`/context-`expand` inputs belong to the caller.
//!
//! Reused, not forked: [`IssueRow`]/[`filter_fields`]/[`FieldSpec`]/
//! [`BASE_EXPANSION_NAMES`] from the sibling [`shape_issue`](super::shape_issue)
//! module, [`user_lite_to_representation`] (D-19, assignees expansion — the
//! same `api/serializers/user.py:13-38` unit),
//! [`state_lite_to_representation`] (`app_project::ser_workflow` — the app and
//! api `StateLiteSerializer` units are the same four fields,
//! `app/serializers/state.py:37-41` vs `api/serializers/state.py:44-55`),
//! and [`CycleReadView`] (D-20 types — `CYCLE_LITE_FIELDS` is order-identical
//! to the fixture's cycle `intended_nested_shape_keys`). Expanded labels
//! arrive pre-rendered: the `LabelLiteSerializer` read shape is
//! PIDASHCONV-661's scope (same seam as [`shape_issue`](super::shape_issue)).
//! No merged `ModuleSerializer` read view exists, so [`ModuleReadView`] is
//! ported inline in D-20 style (manual `Serialize`, annotation-fed metrics).
//!
//! Reachability (all verified against `api/views/` + `api/serializers/`):
//!
//! * `IssueExpandSerializer` — nested only, as `issue_detail` in
//!   `api/serializers/intake.py:65` (`read_only=True, source="issue"`, no
//!   `fields=`/`expand=` kwargs; the intake views pass query `fields=`/
//!   `expand=` to the outer `IntakeIssueSerializer`, which the nested
//!   serializer does not inherit — only `context`). The labels/assignees
//!   branch reads the *context* channel (`self.context.get("expand")`),
//!   unlike `IssueSerializer` which branches on the constructor channel
//!   (`issue.py:443,458`); [`ExpandRepresentationInput`] therefore carries
//!   both `expand` (Base pass) and `context_expand` (method fields).
//! * `issue.py` `CycleIssueSerializer`/`ModuleIssueSerializer` — unreachable:
//!   the package `__init__.py:41,50` exports the `cycle.py:158`/`module.py:209`
//!   classes under the same names, so every view (and every other importer)
//!   resolves to the D-20 pair; nothing imports the `issue.py` pair.
//! * `IssueSearchSerializer` + the advanced family — docs-only: both search
//!   endpoints hand-build their dicts (`views/issue.py:2711-2720`,
//!   `:2859-2915`) and never construct these serializers. The shapes below
//!   are direct-call parity + fixture replay; the handler (PIDASHCONV-677)
//!   renders the wire dicts itself (note the wire divergences: legacy
//!   `sequence_id` is a JSON number on the wire but `"1"` through the
//!   serializer's `CharField`, and the advanced `url` key is *omitted* on
//!   the wire when unconfigured while the serializer renders explicit `null`
//!   for a `None` input).
//!
//! Ported quirks (translate, don't redesign — all verified against the pinned
//! DRF 3.15.2 sources or the fixture):
//!
//! * Expand `cycle`/`module` keys NEVER render — linked or not. `issue_cycle`
//!   and `issue_module` are reverse-FK related managers
//!   (`db/models/cycle.py:109`, `db/models/module.py:154`), so the
//!   `issue_cycle.cycle` / `issue_module.module` source traversal always
//!   raises `AttributeError`, which `Field.get_attribute` turns into
//!   `SkipField` for the non-required read-only fields (`fields.py:431-466`,
//!   pinned DRF 3.15.2). The fixture pins `<KEY ABSENT>` for both the linked
//!   and unlinked probes. The render takes no cycle/module row input at all —
//!   but the names stay real fields for `fields=`/`expand=`: `fields=cycle`
//!   keeps a key that renders nothing, and constructor-`expand=cycle` *adds*
//!   a trailing `"cycle": null` (Base `else` branch over the missing
//!   `cycle_id` attribute, `base.py:114-116`). Same for `module`.
//! * The `issue.py` link pair has no `Meta.model`, so every live render
//!   raises `AssertionError`. Per the F18-03 instruction this port renders
//!   the DECLARED shape (`{cycle: <CycleSerializer>}` /
//!   `{module: <ModuleSerializer>}`) instead of the raise; the classes are
//!   unreachable (shadowed imports, above), so no wire path diverges.
//! * Constructor-`expand` NULLS `labels`/`assignees`/`description` (Base
//!   `else` branch, `base.py:114-116` — `getattr(instance, "<name>_id",
//!   None)` is `None`), *overwriting* the method-field output, while
//!   context-`expand` EXPANDS them to lite objects. Opposite effects, two
//!   channels — both ported.
//! * `?expand=cycle` / `?expand=module` on the link pair replaces the nested
//!   object with the link row's pk: the Base `else` branch reads the
//!   `<name>_id` attribute, which EXISTS on `CycleIssue`/`ModuleIssue`
//!   (their FKs), unlike on `Issue`.
//! * `description` and `description_json` render the same value (the declared
//!   `JSONField(source="description_json")` plus the auto model field).
//! * `ModuleSerializer.members` is declared `write_only=True`
//!   (`module.py:177-181`), hence absent from every read — including the
//!   nested module shape here. F18-03's module `intended_nested_shape_keys`
//!   lists it (model-field inference on an unrenderable class); the replay
//!   test pins the true 22-key list and the deviation is noted in the PR.
//! * Advanced `url` is three-state: input absent → key omitted
//!   (`required=False` + `SkipField`), explicit `None` → `null`
//!   (`allow_null=True`), string → string. [`UrlPresence`] models all three.
//! * Advanced `state` renders `null` both when the input state is `None`
//!   (the `None` shortcut, `serializers.py:530-534`) and when the key is
//!   absent (`allow_null=True` in `Field.get_attribute`). On the wire the
//!   view always passes a dict (possibly `{name: None, group: None}` for
//!   stateless issues), so wire `state` is never `null` — the shape supports
//!   both.
//!
//! JSON rendering notes:
//!
//! * `to_representation` builds a `serde_json::Map` in wire order; key order
//!   is insertion order (`preserve_order`, declared on this crate's
//!   `serde_json` dependency). DRF's compact separators (`(',', ':')`) match
//!   `serde_json` compact output. Byte-exact order assertions in the tests
//!   guard it.
//! * Nested lite/cycle views serialize via `serde` structs in declaration
//!   order (the D-19/D-20 pattern); `serde_json::to_value` on them is
//!   infallible (string keys, no floats except the guarded `sort_order` /
//!   estimate metrics, which map to [`ExpandSearchError::NonFiniteFloat`]
//!   where Django would emit the literal token).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use pidash_types::v1_cycles_modules::cycle_shapes::CycleReadView;
use serde::ser::SerializeStruct;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::app_project::ser_workflow::{state_lite_to_representation, StateLiteRow};
use crate::v1_projects::ser_collab::{user_lite_to_representation, UserLiteRow};

use super::shape_issue::{IssueRow, BASE_EXPANSION_NAMES};
use super::{filter_fields, FieldSpec, FilterError};

/// Failure modes of the renders in this module. None of these are 400 wire
/// bodies — handlers map them (the binary/NaN arms reproduce Django 500s;
/// the `MissingExpansion` arm is unreachable when the handler fetches what
/// the branch needs). Module-local (rather than reusing
/// [`shape_issue`](super::shape_issue)'s error) so [`NonFiniteFloat`](Self::NonFiniteFloat)
/// can name its field (`sort_order` vs `rank` vs nested views).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpandSearchError {
    /// `fields=` filter failure (`TypeError` parity, see [`filter_fields`]).
    #[error("fields filter failed: {0}")]
    Fields(#[from] FilterError),
    /// Non-UTF-8 `description_binary`: Django's `obj.decode()` raises
    /// `UnicodeDecodeError` → 500.
    #[error("description_binary is not valid UTF-8 (Django 500s here)")]
    BinaryNotUtf8,
    /// Non-finite float in the named field: `serde_json` cannot render
    /// NaN/Infinity (Postgres `float8` admits them; Django emits the literal
    /// tokens).
    #[error("{0} is not finite (Django emits the NaN/Infinity literal)")]
    NonFiniteFloat(&'static str),
    /// A map-hit `expand` name with no caller value (Python always renders
    /// the related object, or `{}` when the FK is null).
    #[error("expand '{0}' needs its rendered value (None renders {{}})")]
    MissingExpansion(String),
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::String(text.to_string()),
        None => Value::Null,
    }
}

fn opt_i64(value: Option<i64>) -> Value {
    match value {
        Some(number) => Value::Number(number.into()),
        None => Value::Null,
    }
}

/// `IssueExpandSerializer` fields-dict order (`issue.py:1074-1117` + `Meta`):
/// `[pk] + declared + concrete + relations` (DRF `get_default_field_names`),
/// minus the excluded `workpad`: Base-declared `id`, the six declared fields
/// (`cycle`, `module` — which never *render*, see below — `labels`,
/// `assignees`, `state`, `description`), then the `Issue` model fields in
/// `_meta` order. `cycle`/`module` stay in this list (they are real fields
/// for `fields=`/`expand=` purposes) but the render loop always skips them
/// (the `SkipField` traversal bug, see the module docs).
pub const EXPAND_ALL_FIELDS_IN_ORDER: &[&str] = &[
    "id",
    "cycle",
    "module",
    "labels",
    "assignees",
    "state",
    "description",
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
    "estimate_point",
    "type",
    "assigned_pod",
];

/// `IssueExpandSerializer.to_representation()` input (`issue.py:1074-1118` +
/// the Base passes, `api/serializers/base.py:19-30,72-117`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandRepresentationInput<'a> {
    /// The issue row (reused [`IssueRow`]; `url`/`type_id` extras unused —
    /// this serializer renders neither).
    pub row: &'a IssueRow<'a>,
    /// Raw `description_json` column (never SQL NULL — `default=dict`):
    /// renders under BOTH `description` and `description_json`.
    pub description_json: Value,
    /// Raw `description_stripped` column (`None` = SQL NULL → `null`).
    pub description_stripped: Option<&'a str>,
    /// The state row for the declared nested `StateLiteSerializer`
    /// (`None` = null FK → `null`).
    pub state_row: Option<StateLiteRow<'a>>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    /// Plain names only reach here in practice (comma-split query strings).
    pub fields: Option<&'a [FieldSpec]>,
    /// The constructor `expand=` names in request order (comma-split query
    /// string): the Base expansion pass. Unreached on the wire (the nested
    /// `intake.py:65` use passes no kwargs) — direct-call parity.
    pub expand: &'a [&'a str],
    /// The *context* `expand` names (`self.context.get("expand")`): what
    /// `get_labels`/`get_assignees` branch on. `"labels"`/`"assignees"`
    /// members select the lite-object lists over the pk lists.
    pub context_expand: &'a [&'a str],
    /// `IssueLabel` ids in queryset (`-created_at`) order.
    pub label_ids: &'a [&'a str],
    /// Caller-rendered expanded labels (`LabelLiteSerializer` read shape,
    /// PIDASHCONV-661's scope), in `Label.objects.filter(pk__in=...)` order.
    pub expanded_labels: &'a [Value],
    /// `IssueAssignee` ids in queryset (`-created_at`) order.
    pub assignee_ids: &'a [&'a str],
    /// User rows for context-`expand=assignees`, in
    /// `User.objects.filter(pk__in=...)` order; rendered via the reused
    /// D-19 `UserLite`.
    pub assignee_rows: &'a [UserLiteRow<'a>],
    /// Rendered values for map-hit constructor-`expand` names (`state`,
    /// `project`, `workspace`, `created_by`, `updated_by`, `parent`,
    /// `estimate_point` among this serializer's fields): `Some(value)`
    /// renders the object, `None` renders `{}` (null FK — DRF `SkipField` on
    /// every field). Looked up only for names in [`BASE_EXPANSION_NAMES`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `IssueExpandSerializer.to_representation()` (`issue.py:1074-1118`)
/// over the `BaseSerializer` passes (`api/serializers/base.py:19-30,72-117`).
///
/// Key order is wire order: the kept readable fields in
/// [`EXPAND_ALL_FIELDS_IN_ORDER`] order — `cycle`/`module` never render from
/// the base pass (the always-`SkipField` traversal bug, see the module docs),
/// though constructor-`expand=cycle`/`expand=module` *adds* a trailing `null`
/// key each (the Base `else` branch reads the missing `<name>_id`
/// attribute).
///
/// Method fields branch on the *context* channel (`context_expand`):
/// `"labels"`/`"assignees"` members select the expanded lite lists, else the
/// pk lists (`issue.py:1090-1101`). The constructor-`expand` Base pass runs
/// after (map hit → caller value or `{}`; `type` → the `type_id` no-op
/// overwrite; anything else kept — including `labels`/`assignees`/
/// `description` — → `null`, verbatim from `base.py:114-116`).
pub fn render_expand(
    input: &ExpandRepresentationInput<'_>,
) -> Result<Map<String, Value>, ExpandSearchError> {
    let kept = filter_fields(EXPAND_ALL_FIELDS_IN_ORDER, input.fields)?;
    let kept_contains = |name: &str| kept.iter().any(|kept| kept == name);

    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        // The always-`SkipField` traversal bug: `cycle`/`module` are kept
        // fields that never render (`Field.get_attribute` raises `SkipField`
        // on the `AttributeError`, DRF 3.15.2 `fields.py:431-466`).
        if name == "cycle" || name == "module" {
            continue;
        }
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "labels" => {
                if input.context_expand.contains(&"labels") {
                    Value::Array(input.expanded_labels.to_vec())
                } else {
                    Value::Array(
                        input
                            .label_ids
                            .iter()
                            .map(|id| Value::String(id.to_string()))
                            .collect(),
                    )
                }
            }
            "assignees" => {
                if input.context_expand.contains(&"assignees") {
                    Value::Array(
                        input
                            .assignee_rows
                            .iter()
                            .map(|assignee| {
                                serde_json::to_value(user_lite_to_representation(assignee))
                                    .expect("UserLite view is always serializable")
                            })
                            .collect(),
                    )
                } else {
                    Value::Array(
                        input
                            .assignee_ids
                            .iter()
                            .map(|id| Value::String(id.to_string()))
                            .collect(),
                    )
                }
            }
            // Declared nested `StateLiteSerializer(read_only=True)`: a null
            // FK renders `null` (the `None` shortcut,
            // DRF 3.15.2 `serializers.py:530-534`).
            "state" => match &input.state_row {
                Some(state) => serde_json::to_value(state_lite_to_representation(state))
                    .expect("StateLite view is always serializable"),
                None => Value::Null,
            },
            // Declared `JSONField(source="description_json")` plus the auto
            // model field: the same value under both keys.
            "description" | "description_json" => input.description_json.clone(),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "deleted_at" => opt_str(row.deleted_at),
            "point" => opt_i64(row.point),
            "name" => Value::String(row.name.to_string()),
            "description_html" => Value::String(row.description_html.to_string()),
            "description_stripped" => opt_str(input.description_stripped),
            "description_binary" => match row.description_binary {
                None => Value::Null,
                Some(bytes) => match std::str::from_utf8(bytes) {
                    Ok(text) => Value::String(text.to_string()),
                    Err(_) => return Err(ExpandSearchError::BinaryNotUtf8),
                },
            },
            "priority" => Value::String(row.priority.to_string()),
            "complexity_score" => Value::Number(row.complexity_score.into()),
            "start_date" => opt_str(row.start_date),
            "target_date" => opt_str(row.target_date),
            "sequence_id" => Value::Number(row.sequence_id.into()),
            "sort_order" => match serde_json::Number::from_f64(row.sort_order) {
                Some(number) => Value::Number(number),
                None => return Err(ExpandSearchError::NonFiniteFloat("sort_order")),
            },
            "completed_at" => opt_str(row.completed_at),
            "archived_at" => opt_str(row.archived_at),
            "is_draft" => Value::Bool(row.is_draft),
            "external_source" => opt_str(row.external_source),
            "external_id" => opt_str(row.external_id),
            "git_work_branch" => Value::String(row.git_work_branch.to_string()),
            "created_via" => opt_str(row.created_via),
            "agent_executor" => opt_str(row.agent_executor),
            "created_by" => opt_str(row.created_by),
            "updated_by" => opt_str(row.updated_by),
            "project" => Value::String(row.project.to_string()),
            "workspace" => Value::String(row.workspace.to_string()),
            "parent" => opt_str(row.parent),
            "estimate_point" => opt_str(row.estimate_point),
            // Auto `type` FK field: a null FK renders `null` (same `None`
            // shortcut as `state`).
            "type" => opt_str(row.type_id),
            "assigned_pod" => opt_str(row.assigned_pod),
            // `filter_fields` only ever yields `EXPAND_ALL_FIELDS_IN_ORDER`
            // names; `cycle`/`module` `continue` above, and every other one
            // is matched here.
            _ => unreachable!("render_expand matched every kept field"),
        };
        out.insert(name.clone(), value);
    }

    // Base expansion (`base.py:76-116`).
    for name in input.expand {
        if !kept_contains(name) {
            continue;
        }
        if BASE_EXPANSION_NAMES.contains(name) {
            let found = input.expansions.iter().find(|(key, _)| key == name);
            match found {
                Some((_, Some(value))) => {
                    out.insert(name.to_string(), value.clone());
                }
                Some((_, None)) => {
                    out.insert(name.to_string(), Value::Object(Map::new()));
                }
                None => return Err(ExpandSearchError::MissingExpansion(name.to_string())),
            }
        } else if *name == "type" {
            // The only kept name whose `<name>_id` attribute exists — a
            // no-op overwrite with the same pk.
            out.insert(name.to_string(), opt_str(row.type_id));
        } else {
            // `labels`, `assignees`, `description`, scalars: the `<name>_id`
            // attribute does not exist → `None` → `null`, overwriting the
            // method-field output (`base.py:114-116`).
            out.insert(name.to_string(), Value::Null);
        }
    }

    Ok(out)
}

/// `issue.py` `CycleIssueSerializer.Meta.fields` (`:1043-1044`): the
/// Base-declared `id` is dropped (not listed), so the declared shape is the
/// single `cycle` key (F18-03 `meta_fields`).
pub const CYCLE_ISSUE_FIELDS: &[&str] = &["cycle"];

/// Declared-shape render input for `issue.py` `CycleIssueSerializer`
/// (`:1033-1046`). Live Django raises `AssertionError` (no `Meta.model`);
/// per the F18-03 instruction this renders the declared shape instead (the
/// class is unreachable — shadowed by `cycle.py:158` — so no wire path
/// diverges). No `Debug`/`PartialEq`: the reused D-20 [`CycleReadView`]
/// carries none (D-20 style), and this input mirrors it.
pub struct CycleIssueInput<'a> {
    /// The nested `CycleSerializer` view (reused D-20 [`CycleReadView`]).
    pub cycle: &'a CycleReadView<'a>,
    /// The link row's `cycle_id` pk: what constructor-`expand=cycle` renders
    /// (the Base `else` branch reads the existing `<name>_id` attribute).
    pub cycle_id: Option<&'a str>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The constructor `expand=` names — direct-call parity only (the class
    /// is never constructed on the wire).
    pub expand: &'a [&'a str],
}

/// Port of the `issue.py` `CycleIssueSerializer` DECLARED read shape
/// (`:1033-1046`): `{cycle: <CycleSerializer>}`. With unannotated metrics
/// the nested object is the fixture's 22-key `intended_nested_shape_keys`.
pub fn render_cycle_issue(
    input: &CycleIssueInput<'_>,
) -> Result<Map<String, Value>, ExpandSearchError> {
    let kept = filter_fields(CYCLE_ISSUE_FIELDS, input.fields)?;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        debug_assert_eq!(name, "cycle", "only CYCLE_ISSUE_FIELDS names survive");
        let nested = serde_json::to_value(input.cycle)
            .map_err(|_| ExpandSearchError::NonFiniteFloat("cycle.sort_order"))?;
        out.insert(name.clone(), nested);
    }

    // Base expansion (`base.py:76-116`): `cycle` is not in the expansion
    // map, so a kept `expand=cycle` renders the link row's `cycle_id` pk.
    for name in input.expand {
        if *name == "cycle" && kept.iter().any(|kept| kept == "cycle") {
            out.insert("cycle".to_string(), opt_str(input.cycle_id));
        }
    }

    Ok(out)
}

/// The 6 declared metric fields on `ModuleSerializer` (`module.py:182-187`),
/// all `read_only=True`, in declaration order. They lead the wire order:
/// DRF emits declared fields before model fields (same rule as D-20's
/// `CYCLE_METRIC_FIELDS`).
pub const MODULE_METRIC_FIELDS: [&str; 6] = [
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
];

/// `ModuleSerializer` wire order, all 28 keys: `id`
/// (`BaseSerializer`, `api/serializers/base.py:17`), the 6 declared metrics,
/// then the `Module` model fields — concrete fields first, then forward
/// relations in model order (`db/models/module.py:66-95`). The declared
/// `members` field is `write_only=True` (`module.py:177-181`), hence absent
/// from every read (this list included).
pub const MODULE_READ_FIELDS: [&str; 28] = [
    "id",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "lead",
];

/// A metric annotation value: `None` = annotation missing (key omitted);
/// `Some(None)` = present but SQL `NULL` (key present, `null`);
/// `Some(Some(v))` = rendered value. Mirrors D-20's `Metric` (a cycle-named
/// alias reads wrong here, so this module carries its own).
pub type ModuleMetric<T> = Option<Option<T>>;

/// `ModuleSerializer` output shape (`module.py:169-197`).
///
/// Wire order is [`MODULE_READ_FIELDS`]. The 6 metrics are annotation-fed
/// (key omitted when the annotation is missing); every other key is always
/// present. Plain FKs render as pk strings (`PrimaryKeyRelatedField`,
/// read-only); a null FK renders `null`. Datetimes/dates cross this boundary
/// already rendered as DRF strings.
pub struct ModuleReadView<'a> {
    /// `BaseSerializer.id` (`api/serializers/base.py:17`).
    pub id: &'a str,
    pub total_issues: ModuleMetric<i64>,
    pub cancelled_issues: ModuleMetric<i64>,
    pub completed_issues: ModuleMetric<i64>,
    pub started_issues: ModuleMetric<i64>,
    pub unstarted_issues: ModuleMetric<i64>,
    pub backlog_issues: ModuleMetric<i64>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<Value>,
    pub description_html: Option<Value>,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub status: &'a str,
    pub view_props: Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub logo_props: Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub lead: Option<&'a str>,
}

fn opt_module_metric<S, T>(
    out: &mut <S as serde::Serializer>::SerializeStruct,
    key: &'static str,
    metric: ModuleMetric<T>,
) -> Result<(), <S as serde::Serializer>::Error>
where
    S: serde::Serializer,
    T: Serialize,
{
    if let Some(inner) = metric {
        match inner {
            Some(v) => out.serialize_field(key, &v)?,
            None => out.serialize_field(key, &Value::Null)?,
        }
    }
    Ok(())
}

fn opt_module_string<S>(
    out: &mut <S as serde::Serializer>::SerializeStruct,
    key: &'static str,
    value: Option<&str>,
) -> Result<(), <S as serde::Serializer>::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => out.serialize_field(key, v),
        None => out.serialize_field(key, &Value::Null),
    }
}

fn opt_module_json<S>(
    out: &mut <S as serde::Serializer>::SerializeStruct,
    key: &'static str,
    value: &Option<Value>,
) -> Result<(), <S as serde::Serializer>::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => out.serialize_field(key, v),
        None => out.serialize_field(key, &Value::Null),
    }
}

impl Serialize for ModuleReadView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("ModuleReadView", 28)?;
        out.serialize_field("id", self.id)?;
        opt_module_metric::<S, i64>(&mut out, "total_issues", self.total_issues)?;
        opt_module_metric::<S, i64>(&mut out, "cancelled_issues", self.cancelled_issues)?;
        opt_module_metric::<S, i64>(&mut out, "completed_issues", self.completed_issues)?;
        opt_module_metric::<S, i64>(&mut out, "started_issues", self.started_issues)?;
        opt_module_metric::<S, i64>(&mut out, "unstarted_issues", self.unstarted_issues)?;
        opt_module_metric::<S, i64>(&mut out, "backlog_issues", self.backlog_issues)?;
        out.serialize_field("created_at", self.created_at)?;
        out.serialize_field("updated_at", self.updated_at)?;
        opt_module_string::<S>(&mut out, "deleted_at", self.deleted_at)?;
        out.serialize_field("name", self.name)?;
        out.serialize_field("description", self.description)?;
        opt_module_json::<S>(&mut out, "description_text", &self.description_text)?;
        opt_module_json::<S>(&mut out, "description_html", &self.description_html)?;
        opt_module_string::<S>(&mut out, "start_date", self.start_date)?;
        opt_module_string::<S>(&mut out, "target_date", self.target_date)?;
        out.serialize_field("status", self.status)?;
        out.serialize_field("view_props", &self.view_props)?;
        out.serialize_field("sort_order", &self.sort_order)?;
        opt_module_string::<S>(&mut out, "external_source", self.external_source)?;
        opt_module_string::<S>(&mut out, "external_id", self.external_id)?;
        opt_module_string::<S>(&mut out, "archived_at", self.archived_at)?;
        out.serialize_field("logo_props", &self.logo_props)?;
        opt_module_string::<S>(&mut out, "created_by", self.created_by)?;
        opt_module_string::<S>(&mut out, "updated_by", self.updated_by)?;
        out.serialize_field("project", self.project)?;
        out.serialize_field("workspace", self.workspace)?;
        opt_module_string::<S>(&mut out, "lead", self.lead)?;
        out.end()
    }
}

/// `issue.py` `ModuleIssueSerializer.Meta.fields` (`:1057-1058`): the
/// Base-declared `id` is dropped (not listed), so the declared shape is the
/// single `module` key (F18-03 `meta_fields`).
pub const MODULE_ISSUE_FIELDS: &[&str] = &["module"];

/// Declared-shape render input for `issue.py` `ModuleIssueSerializer`
/// (`:1047-1060`). Same unreachable-but-declared status as
/// [`CycleIssueInput`] (shadowed by `module.py:209`), and the same
/// derive-less shape (mirrors the D-20 view style).
pub struct ModuleIssueInput<'a> {
    /// The nested `ModuleSerializer` view ([`ModuleReadView`], inline port).
    pub module: &'a ModuleReadView<'a>,
    /// The link row's `module_id` pk: what constructor-`expand=module`
    /// renders (the Base `else` branch reads the existing `<name>_id`
    /// attribute).
    pub module_id: Option<&'a str>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The constructor `expand=` names — direct-call parity only.
    pub expand: &'a [&'a str],
}

/// Port of the `issue.py` `ModuleIssueSerializer` DECLARED read shape
/// (`:1047-1060`): `{module: <ModuleSerializer>}`. With unannotated metrics
/// the nested object is the true 22-key read list (F18-03's
/// `intended_nested_shape_keys` minus the `write_only` `members` — see the
/// module docs).
pub fn render_module_issue(
    input: &ModuleIssueInput<'_>,
) -> Result<Map<String, Value>, ExpandSearchError> {
    let kept = filter_fields(MODULE_ISSUE_FIELDS, input.fields)?;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        debug_assert_eq!(name, "module", "only MODULE_ISSUE_FIELDS names survive");
        let nested = serde_json::to_value(input.module)
            .map_err(|_| ExpandSearchError::NonFiniteFloat("module.sort_order"))?;
        out.insert(name.clone(), nested);
    }

    // Base expansion (`base.py:76-116`): `module` is not in the expansion
    // map, so a kept `expand=module` renders the link row's `module_id` pk.
    for name in input.expand {
        if *name == "module" && kept.iter().any(|kept| kept == "module") {
            out.insert("module".to_string(), opt_str(input.module_id));
        }
    }

    Ok(out)
}

/// `IssueSearchSerializer` field order (`issue.py:1147-1152`): six required
/// `CharField`s. Field errors combine in this order.
pub const SEARCH_FIELDS: [&str; 6] = [
    "id",
    "name",
    "sequence_id",
    "project__identifier",
    "project_id",
    "workspace__slug",
];

/// A row for `IssueSearchSerializer.to_representation` (`issue.py:1139-1153`).
/// Ids cross this boundary already stringified; `sequence_id` stays numeric
/// so the render ports the `CharField` `str()` coercion the fixture pins
/// (`1` → `"1"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueSearchRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub sequence_id: i64,
    pub project_identifier: &'a str,
    pub project_id: &'a str,
    pub workspace_slug: &'a str,
}

/// Port of `IssueSearchSerializer.to_representation` (`issue.py:1139-1153`):
/// all six keys always present, all strings (`CharField.to_representation`
/// is `str(value)`, DRF 3.15.2 `fields.py:768-769`).
pub fn render_issue_search(row: &IssueSearchRow<'_>) -> Map<String, Value> {
    let mut out = Map::with_capacity(SEARCH_FIELDS.len());
    out.insert("id".to_string(), Value::String(row.id.to_string()));
    out.insert("name".to_string(), Value::String(row.name.to_string()));
    out.insert(
        "sequence_id".to_string(),
        Value::String(row.sequence_id.to_string()),
    );
    out.insert(
        "project__identifier".to_string(),
        Value::String(row.project_identifier.to_string()),
    );
    out.insert(
        "project_id".to_string(),
        Value::String(row.project_id.to_string()),
    );
    out.insert(
        "workspace__slug".to_string(),
        Value::String(row.workspace_slug.to_string()),
    );
    out
}

/// One `IssueSearchSerializer` field failure: the JSON wire message plus the
/// DRF error code (the fixture records both).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchFieldError {
    /// One of [`SEARCH_FIELDS`], in field order.
    pub field: &'static str,
    /// The wire message (e.g. `"This field is required."`).
    pub message: &'static str,
    /// The DRF code (e.g. `"required"`).
    pub code: &'static str,
}

/// `IssueSearchSerializer.validated_data` (`issue.py:1139-1153`): every value
/// coerced and whitespace-trimmed (`CharField` `trim_whitespace=True`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueSearchValidated {
    pub id: String,
    pub name: String,
    pub sequence_id: String,
    pub project_identifier: String,
    pub project_id: String,
    pub workspace_slug: String,
}

/// Port of `IssueSearchSerializer` field validation (`issue.py:1139-1153`
/// over DRF 3.15.2 `CharField.run_validation`/`to_internal_value`,
/// `fields.py:749-769`, plus the null-character validator). All six fields
/// are required, non-blank, non-null plain `CharField`s; unknown input keys
/// are ignored (plain `Serializer`). Errors combine in [`SEARCH_FIELDS`]
/// order.
///
/// Rule chain per field, in DRF order: absent → `required`; `null` → `null`;
/// bool/array/object → `invalid` (`"Not a valid string."`); `""` or
/// whitespace-only → `blank`; numbers → `str()`; strings → stripped, then
/// the `\x00` check (`"Null characters are not allowed."`). Surrogate halves
/// are unrepresentable in Rust `&str` (`serde_json` rejects them at parse),
/// so the surrogate validator arm is unreachable here.
pub fn validate_issue_search(
    input: &Map<String, Value>,
) -> Result<IssueSearchValidated, Vec<SearchFieldError>> {
    fn one(field: &'static str, input: &Map<String, Value>) -> Result<String, SearchFieldError> {
        let fail = |message: &'static str, code: &'static str| SearchFieldError {
            field,
            message,
            code,
        };
        let Some(value) = input.get(field) else {
            return Err(fail("This field is required.", "required"));
        };
        match value {
            Value::Null => Err(fail("This field may not be null.", "null")),
            Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
                Err(fail("Not a valid string.", "invalid"))
            }
            // `str(data)` coercion (`fields.py:765`): a JSON number's
            // shortest round-trip spells exactly what Python `str()` spells
            // for the same value, and is never blank.
            Value::Number(number) => Ok(number.to_string()),
            Value::String(text) => {
                // `data == '' or str(data).strip() == ''` (`fields.py:753`);
                // `str::trim` is Unicode whitespace like Python's `strip`
                // except for `\x1c`-`\x1f` (stripped by Python, kept by
                // Rust) — no fixture input exercises the difference.
                if text.trim().is_empty() {
                    return Err(fail("This field may not be blank.", "blank"));
                }
                let trimmed = text.trim();
                if trimmed.contains('\0') {
                    return Err(fail(
                        "Null characters are not allowed.",
                        "null_characters_not_allowed",
                    ));
                }
                Ok(trimmed.to_string())
            }
        }
    }

    let mut errors = Vec::new();
    let mut get = |field: &'static str| match one(field, input) {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    let id = get("id");
    let name = get("name");
    let sequence_id = get("sequence_id");
    let project_identifier = get("project__identifier");
    let project_id = get("project_id");
    let workspace_slug = get("workspace__slug");
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(IssueSearchValidated {
        id: id.unwrap_or_default(),
        name: name.unwrap_or_default(),
        sequence_id: sequence_id.unwrap_or_default(),
        project_identifier: project_identifier.unwrap_or_default(),
        project_id: project_id.unwrap_or_default(),
        workspace_slug: workspace_slug.unwrap_or_default(),
    })
}

/// The exact 400 body for [`validate_issue_search`] failures:
/// `{field: [message]}` in [`SEARCH_FIELDS`] order with DRF compact
/// separators.
pub fn search_errors_body(errors: &[SearchFieldError]) -> String {
    let mut out = Map::with_capacity(errors.len());
    for error in errors {
        out.insert(
            error.field.to_string(),
            Value::Array(vec![Value::String(error.message.to_string())]),
        );
    }
    serde_json::to_string(&out).expect("string-keyed map with string values serializes")
}

/// `IssueAdvancedSearchStateSerializer` output (`issue.py:1155-1158`):
/// `{name, group}`, each `null` when the input is `None` (`allow_null=True`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdvancedSearchStateView<'a> {
    pub name: Option<&'a str>,
    pub group: Option<&'a str>,
}

/// `IssueAdvancedSearchProjectSerializer` output (`issue.py:1160-1163`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdvancedSearchProjectView<'a> {
    pub id: &'a str,
    pub identifier: &'a str,
    pub name: &'a str,
}

/// The advanced-result `url` field (`issue.py:1190-1197`,
/// `required=False, allow_null=True`): input absent → key omitted
/// (`SkipField`); explicit `None` → `null`; string → string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlPresence<'a> {
    /// Key absent from the input: the key is omitted from the output.
    Absent,
    /// Explicit `None` input: the key renders `null`.
    Null,
    /// A URL string.
    Value(&'a str),
}

/// A row for `IssueAdvancedSearchResultSerializer.to_representation`
/// (`issue.py:1166-1198`). Datetimes cross this boundary already rendered as
/// DRF strings; `rank` is the `float(row["_rank"] or 0.0)` value.
#[derive(Debug, Clone, PartialEq)]
pub struct AdvancedSearchResultRow<'a> {
    pub id: &'a str,
    pub sequence_id: i64,
    pub identifier: &'a str,
    pub name: &'a str,
    /// `None` renders `null` (`allow_blank=True, allow_null=True`).
    pub snippet: Option<&'a str>,
    /// `None` renders `null` (the `None` shortcut); `Some` renders the
    /// `{name, group}` object.
    pub state: Option<AdvancedSearchStateView<'a>>,
    pub project: AdvancedSearchProjectView<'a>,
    pub workspace_slug: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    /// `None` renders `null` (`allow_null=True`).
    pub completed_at: Option<&'a str>,
    pub rank: f64,
    pub url: UrlPresence<'a>,
}

/// `IssueAdvancedSearchResultSerializer` field order (`issue.py:1174-1197`).
pub const ADVANCED_RESULT_FIELDS: [&str; 13] = [
    "id",
    "sequence_id",
    "identifier",
    "name",
    "snippet",
    "state",
    "project",
    "workspace_slug",
    "created_at",
    "updated_at",
    "completed_at",
    "rank",
    "url",
];

/// Port of `IssueAdvancedSearchResultSerializer.to_representation`
/// (`issue.py:1166-1198`): the 13 keys in declaration order, `url` omitted
/// only for [`UrlPresence::Absent`].
pub fn render_advanced_result(
    row: &AdvancedSearchResultRow<'_>,
) -> Result<Map<String, Value>, ExpandSearchError> {
    let mut out = Map::with_capacity(ADVANCED_RESULT_FIELDS.len());
    out.insert("id".to_string(), Value::String(row.id.to_string()));
    out.insert(
        "sequence_id".to_string(),
        Value::Number(row.sequence_id.into()),
    );
    out.insert(
        "identifier".to_string(),
        Value::String(row.identifier.to_string()),
    );
    out.insert("name".to_string(), Value::String(row.name.to_string()));
    out.insert("snippet".to_string(), opt_str(row.snippet));
    out.insert(
        "state".to_string(),
        match &row.state {
            Some(state) => serde_json::to_value(state)
                .expect("state view (strings/options only) is always serializable"),
            None => Value::Null,
        },
    );
    out.insert(
        "project".to_string(),
        serde_json::to_value(&row.project)
            .expect("project view (strings only) is always serializable"),
    );
    out.insert(
        "workspace_slug".to_string(),
        Value::String(row.workspace_slug.to_string()),
    );
    out.insert(
        "created_at".to_string(),
        Value::String(row.created_at.to_string()),
    );
    out.insert(
        "updated_at".to_string(),
        Value::String(row.updated_at.to_string()),
    );
    out.insert("completed_at".to_string(), opt_str(row.completed_at));
    out.insert(
        "rank".to_string(),
        match serde_json::Number::from_f64(row.rank) {
            Some(number) => Value::Number(number),
            None => return Err(ExpandSearchError::NonFiniteFloat("rank")),
        },
    );
    match row.url {
        UrlPresence::Absent => {}
        UrlPresence::Null => {
            out.insert("url".to_string(), Value::Null);
        }
        UrlPresence::Value(url) => {
            out.insert("url".to_string(), Value::String(url.to_string()));
        }
    }
    Ok(out)
}

/// Port of `IssueAdvancedSearchResponseSerializer.to_representation`
/// (`issue.py:1200-1203`): `{query, count, results}` in declaration order.
pub fn render_advanced_response(
    query: &str,
    count: i64,
    results: Vec<Map<String, Value>>,
) -> Map<String, Value> {
    let mut out = Map::with_capacity(3);
    out.insert("query".to_string(), Value::String(query.to_string()));
    out.insert("count".to_string(), Value::Number(count.into()));
    out.insert(
        "results".to_string(),
        Value::Array(results.into_iter().map(Value::Object).collect()),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const F18_03: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/serializers/",
        "F18-03.comment_attachment_activity_expand_search.golden.json"
    );

    fn fixture() -> Value {
        let raw = std::fs::read_to_string(F18_03).expect("F18-03 golden exists");
        serde_json::from_str(&raw).expect("F18-03 golden is valid JSON")
    }

    fn unit<'a>(fx: &'a Value, name: &str) -> &'a Value {
        fx.pointer(&format!("/units/{name}"))
            .unwrap_or_else(|| panic!("F18-03 lacks units.{name}"))
    }

    fn str_list(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("golden carries a string list")
            .iter()
            .map(|item| item.as_str().expect("entries are strings"))
            .collect()
    }

    fn map_keys(map: &Map<String, Value>) -> Vec<String> {
        map.keys().cloned().collect()
    }

    fn rendered(map: &Map<String, Value>) -> String {
        serde_json::to_string(map).expect("rendered map serializes")
    }

    fn issue_row() -> IssueRow<'static> {
        IssueRow {
            id: "a7509d00-345f-47fb-bee3-6bcf7d3339e2",
            type_id: Some("d615b3e1-98e0-4f0e-9a1c-2b2e2e2e2e2e"),
            url: None,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-02T00:00:00Z",
            deleted_at: None,
            point: Some(3),
            name: "First issue",
            description_html: "<p>hi</p>",
            description_binary: None,
            priority: "high",
            complexity_score: 5,
            start_date: Some("2026-01-03"),
            target_date: None,
            sequence_id: 1,
            sort_order: 65535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            git_work_branch: "",
            created_via: None,
            agent_executor: None,
            created_by: Some("79c81d76-5a93-4d3d-894d-5935576834b6"),
            updated_by: Some("79c81d76-5a93-4d3d-894d-5935576834b6"),
            project: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            workspace: "e8f1a2b3-1111-4222-8333-444444444444",
            parent: None,
            state: Some("97b22834-b823-4109-a527-b39aa310ceae"),
            estimate_point: None,
            assigned_pod: None,
        }
    }

    fn expand_input<'a>(row: &'a IssueRow<'a>) -> ExpandRepresentationInput<'a> {
        ExpandRepresentationInput {
            row,
            description_json: json!({}),
            description_stripped: Some("hi"),
            state_row: Some(StateLiteRow {
                id: "97b22834-b823-4109-a527-b39aa310ceae",
                name: "Todo",
                color: "#ff0000",
                group: "unstarted",
            }),
            fields: None,
            expand: &[],
            context_expand: &[],
            label_ids: &["c833c492-fa3d-49f5-85a4-0ef810642731"],
            expanded_labels: &[],
            assignee_ids: &["79c81d76-5a93-4d3d-894d-5935576834b6"],
            assignee_rows: &[],
            expansions: &[],
        }
    }

    /// Owned expanded-label values (the F18-03 `labels_expanded` golden);
    /// call sites bind these so the borrowed input outlives the render.
    fn expanded_label_values() -> Vec<Value> {
        vec![json!({
            "id": "c833c492-fa3d-49f5-85a4-0ef810642731",
            "name": "bug",
            "color": "",
        })]
    }

    /// Owned assignee user rows (the F18-03 `assignees_expanded` golden
    /// inputs); call sites bind these so the borrowed input outlives the
    /// render.
    fn assignee_user_rows() -> Vec<UserLiteRow<'static>> {
        vec![UserLiteRow {
            id: "79c81d76-5a93-4d3d-894d-5935576834b6",
            first_name: "",
            last_name: "",
            email: Some("conv659-owner@example.com"),
            avatar: "",
            avatar_url: None,
            display_name: "",
        }]
    }

    #[test]
    fn expand_keys_match_golden_render_keys() {
        let fx = fixture();
        let row = issue_row();
        let rendered_map = render_expand(&expand_input(&row)).expect("renders");
        assert_eq!(
            map_keys(&rendered_map),
            str_list(&unit(&fx, "IssueExpandSerializer")["render_keys"])
        );
        // The always-absent traversal bug: no cycle/module keys, and the
        // excluded workpad never appears.
        for key in ["cycle", "module", "workpad", "url", "type_id"] {
            assert!(!rendered_map.contains_key(key), "no {key} key");
        }
        assert_eq!(rendered_map.len(), 36);
    }

    #[test]
    fn expand_labels_plain_and_expanded_match_golden() {
        let fx = fixture();
        let golden = unit(&fx, "IssueExpandSerializer");
        let row = issue_row();
        let plain = render_expand(&expand_input(&row)).expect("renders");
        assert_eq!(
            serde_json::to_string(&plain["labels"]).expect("serializes"),
            serde_json::to_string(&golden["labels_plain"]).expect("serializes"),
        );
        let labels = expanded_label_values();
        let mut expanded = expand_input(&row);
        expanded.context_expand = &["labels"];
        expanded.expanded_labels = &labels;
        let rendered_map = render_expand(&expanded).expect("renders");
        assert_eq!(
            serde_json::to_string(&rendered_map["labels"]).expect("serializes"),
            serde_json::to_string(&golden["labels_expanded"]).expect("serializes"),
        );
    }

    #[test]
    fn expand_assignees_plain_and_expanded_match_golden() {
        let fx = fixture();
        let golden = unit(&fx, "IssueExpandSerializer");
        let row = issue_row();
        let plain = render_expand(&expand_input(&row)).expect("renders");
        assert_eq!(
            serde_json::to_string(&plain["assignees"]).expect("serializes"),
            serde_json::to_string(&golden["assignees_plain"]).expect("serializes"),
        );
        let rows = assignee_user_rows();
        let mut expanded = expand_input(&row);
        expanded.context_expand = &["assignees"];
        expanded.assignee_rows = &rows;
        let rendered_map = render_expand(&expanded).expect("renders");
        assert_eq!(
            serde_json::to_string(&rendered_map["assignees"]).expect("serializes"),
            serde_json::to_string(&golden["assignees_expanded"]).expect("serializes"),
        );
    }

    #[test]
    fn expand_state_and_description_match_golden() {
        let fx = fixture();
        let golden = unit(&fx, "IssueExpandSerializer");
        let row = issue_row();
        let rendered_map = render_expand(&expand_input(&row)).expect("renders");
        assert_eq!(
            serde_json::to_string(&rendered_map["state"]).expect("serializes"),
            serde_json::to_string(&golden["state_shape"]).expect("serializes"),
        );
        // Both `description` and `description_json` render the same value.
        assert_eq!(
            serde_json::to_string(&rendered_map["description"]).expect("serializes"),
            serde_json::to_string(&golden["description_key"]).expect("serializes"),
        );
        assert_eq!(
            rendered_map["description"],
            rendered_map["description_json"]
        );
        assert_eq!(golden["has_workpad"], Value::Bool(false));
    }

    #[test]
    fn expand_fields_filter_and_constructor_expand_parity() {
        let row = issue_row();
        // `fields=` keeps a wire-ordered subset.
        let fields = vec![
            FieldSpec::Include("name".to_string()),
            FieldSpec::Include("id".to_string()),
        ];
        let mut input = expand_input(&row);
        input.fields = Some(&fields);
        let rendered_map = render_expand(&input).expect("renders");
        assert_eq!(map_keys(&rendered_map), vec!["id", "name"]);

        // Constructor-`expand=labels` NULLS labels (Base `else` branch),
        // even when the context channel expanded them.
        let mut input = expand_input(&row);
        input.context_expand = &["labels"];
        input.expand = &["labels"];
        let rendered_map = render_expand(&input).expect("renders");
        assert_eq!(rendered_map["labels"], Value::Null);

        // Constructor-`expand=state` on a null FK renders `{}`.
        let mut input = expand_input(&row);
        input.state_row = None;
        input.expand = &["state"];
        input.expansions = &[("state", None)];
        let rendered_map = render_expand(&input).expect("renders");
        assert_eq!(rendered_map["state"], Value::Object(Map::new()));

        // ... while without the Base pass a null FK renders `null`.
        let mut input = expand_input(&row);
        input.state_row = None;
        let rendered_map = render_expand(&input).expect("renders");
        assert_eq!(rendered_map["state"], Value::Null);

        // `expand=type` is the no-op overwrite (the `<name>_id` attribute
        // exists for `type`).
        let mut input = expand_input(&row);
        input.expand = &["type"];
        let rendered_map = render_expand(&input).expect("renders");
        assert_eq!(
            rendered_map["type"],
            Value::String("d615b3e1-98e0-4f0e-9a1c-2b2e2e2e2e2e".to_string()),
        );

        // `cycle`/`module` are real fields for `fields=` but render nothing.
        let fields = vec![FieldSpec::Include("cycle".to_string())];
        let mut input = expand_input(&row);
        input.fields = Some(&fields);
        assert!(render_expand(&input).expect("renders").is_empty());

        // ... while constructor-`expand=cycle` *adds* a trailing null key.
        let mut input = expand_input(&row);
        input.expand = &["cycle", "module"];
        let rendered_map = render_expand(&input).expect("renders");
        assert_eq!(rendered_map.len(), 38);
        assert_eq!(rendered_map["cycle"], Value::Null);
        assert_eq!(rendered_map["module"], Value::Null);

        // Map-hit `expand` with no caller value is a caller-contract error.
        let mut input = expand_input(&row);
        input.expand = &["state"];
        assert_eq!(
            render_expand(&input),
            Err(ExpandSearchError::MissingExpansion("state".to_string())),
        );

        // A nested `fields=` entry raises (TypeError parity).
        let nested = vec![FieldSpec::Nested("state".to_string(), vec![])];
        let mut input = expand_input(&row);
        input.fields = Some(&nested);
        assert!(matches!(
            render_expand(&input),
            Err(ExpandSearchError::Fields(_))
        ));
    }

    #[test]
    fn expand_scalar_and_error_arms() {
        let row = issue_row();
        let rendered_map = render_expand(&expand_input(&row)).expect("renders");
        assert_eq!(rendered_map["point"], json!(3));
        assert_eq!(rendered_map["complexity_score"], json!(5));
        assert_eq!(rendered_map["sequence_id"], json!(1));
        assert_eq!(rendered_map["sort_order"], json!(65535.0));
        assert_eq!(rendered_map["is_draft"], Value::Bool(false));
        assert_eq!(rendered_map["description_binary"], Value::Null);
        assert_eq!(
            rendered_map["type"],
            Value::String("d615b3e1-98e0-4f0e-9a1c-2b2e2e2e2e2e".to_string()),
        );

        // Non-UTF-8 binary 500s in Django and errors here.
        let mut bad_row = issue_row();
        bad_row.description_binary = Some(&[0xff, 0xfe]);
        assert_eq!(
            render_expand(&expand_input(&bad_row)),
            Err(ExpandSearchError::BinaryNotUtf8),
        );

        // Non-finite sort_order errors here.
        let mut bad_row = issue_row();
        bad_row.sort_order = f64::NAN;
        assert_eq!(
            render_expand(&expand_input(&bad_row)),
            Err(ExpandSearchError::NonFiniteFloat("sort_order")),
        );
    }

    fn cycle_view() -> CycleReadView<'static> {
        CycleReadView {
            id: "c10e0000-0000-4000-8000-000000000001",
            total_issues: None,
            cancelled_issues: None,
            completed_issues: None,
            started_issues: None,
            unstarted_issues: None,
            backlog_issues: None,
            total_estimates: None,
            completed_estimates: None,
            started_estimates: None,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-02T00:00:00Z",
            deleted_at: None,
            name: "Sprint 1",
            description: "",
            start_date: None,
            end_date: None,
            view_props: json!({}),
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            progress_snapshot: json!({}),
            archived_at: None,
            logo_props: json!({}),
            timezone: "UTC",
            version: 1,
            created_by: None,
            updated_by: None,
            project: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            workspace: "e8f1a2b3-1111-4222-8333-444444444444",
            owned_by: "79c81d76-5a93-4d3d-894d-5935576834b6",
        }
    }

    #[test]
    fn cycle_issue_renders_declared_shape() {
        let fx = fixture();
        let golden = unit(&fx, "CycleIssueSerializer");
        assert_eq!(str_list(&golden["meta_fields"]), CYCLE_ISSUE_FIELDS);
        assert!(!golden["has_meta_model"].as_bool().expect("bool"));

        let view = cycle_view();
        let input = CycleIssueInput {
            cycle: &view,
            cycle_id: Some("c10e0000-0000-4000-8000-000000000001"),
            fields: None,
            expand: &[],
        };
        let rendered_map = render_cycle_issue(&input).expect("renders");
        assert_eq!(map_keys(&rendered_map), vec!["cycle"]);
        let nested = rendered_map["cycle"].as_object().expect("nested object");
        // Unannotated metrics drop their keys, leaving exactly the
        // fixture's intended 22-key list in order.
        assert_eq!(
            map_keys(nested),
            str_list(&golden["intended_nested_shape_keys"])
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(nested["name"], Value::String("Sprint 1".to_string()));

        // `expand=cycle` replaces the object with the link row's pk.
        let expanded = CycleIssueInput {
            cycle: &view,
            cycle_id: Some("c10e0000-0000-4000-8000-000000000001"),
            fields: None,
            expand: &["cycle"],
        };
        let rendered_map = render_cycle_issue(&expanded).expect("renders");
        assert_eq!(
            rendered_map["cycle"],
            Value::String("c10e0000-0000-4000-8000-000000000001".to_string()),
        );
    }

    fn module_view() -> ModuleReadView<'static> {
        ModuleReadView {
            id: "d20e0000-0000-4000-8000-000000000002",
            total_issues: None,
            cancelled_issues: None,
            completed_issues: None,
            started_issues: None,
            unstarted_issues: None,
            backlog_issues: None,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-02T00:00:00Z",
            deleted_at: None,
            name: "Auth",
            description: "",
            description_text: None,
            description_html: None,
            start_date: None,
            target_date: None,
            status: "planned",
            view_props: json!({}),
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            archived_at: None,
            logo_props: json!({}),
            created_by: None,
            updated_by: None,
            project: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            workspace: "e8f1a2b3-1111-4222-8333-444444444444",
            lead: None,
        }
    }

    #[test]
    fn module_issue_renders_declared_shape() {
        let fx = fixture();
        let golden = unit(&fx, "ModuleIssueSerializer");
        assert_eq!(str_list(&golden["meta_fields"]), MODULE_ISSUE_FIELDS);
        assert!(!golden["has_meta_model"].as_bool().expect("bool"));

        let view = module_view();
        let input = ModuleIssueInput {
            module: &view,
            module_id: Some("d20e0000-0000-4000-8000-000000000002"),
            fields: None,
            expand: &[],
        };
        let rendered_map = render_module_issue(&input).expect("renders");
        assert_eq!(map_keys(&rendered_map), vec!["module"]);
        let nested = rendered_map["module"].as_object().expect("nested object");

        // True read list: MODULE_READ_FIELDS minus the unannotated metrics.
        let expected: Vec<String> = MODULE_READ_FIELDS
            .iter()
            .filter(|key| !MODULE_METRIC_FIELDS.contains(key))
            .map(ToString::to_string)
            .collect();
        assert_eq!(map_keys(nested), expected);
        assert_eq!(nested.len(), 22);
        assert_eq!(nested["status"], Value::String("planned".to_string()));

        // The ONLY deviation from F18-03's inferred intended list is the
        // `write_only` `members` key (`module.py:177-181`), which live DRF
        // never renders.
        let intended: Vec<String> = str_list(&golden["intended_nested_shape_keys"])
            .iter()
            .map(ToString::to_string)
            .collect();
        let missing: Vec<&String> = intended
            .iter()
            .filter(|key| !nested.contains_key(*key))
            .collect();
        assert_eq!(missing, vec!["members"]);
        assert_eq!(intended.len(), 23);

        // `expand=module` replaces the object with the link row's pk.
        let expanded = ModuleIssueInput {
            module: &view,
            module_id: Some("d20e0000-0000-4000-8000-000000000002"),
            fields: None,
            expand: &["module"],
        };
        let rendered_map = render_module_issue(&expanded).expect("renders");
        assert_eq!(
            rendered_map["module"],
            Value::String("d20e0000-0000-4000-8000-000000000002".to_string()),
        );
    }

    #[test]
    fn module_metrics_follow_annotation_rule() {
        let mut view = module_view();
        view.total_issues = Some(Some(4));
        view.backlog_issues = Some(None);
        let rendered: Value = serde_json::to_value(&view).expect("module view serializes");
        let nested = rendered.as_object().expect("object");
        // Present annotation renders, NULL annotation renders null, missing
        // annotations drop their keys.
        assert_eq!(nested["total_issues"], json!(4));
        assert_eq!(nested["backlog_issues"], Value::Null);
        for key in [
            "cancelled_issues",
            "completed_issues",
            "started_issues",
            "unstarted_issues",
        ] {
            assert!(!nested.contains_key(key), "missing annotation drops {key}");
        }
        // Full wire order with every metric present.
        assert_eq!(MODULE_READ_FIELDS.len(), 28);
    }

    #[test]
    fn search_render_matches_golden_byte_identical() {
        let fx = fixture();
        let golden = unit(&fx, "IssueSearchSerializer");
        let row = IssueSearchRow {
            id: "a7509d00-345f-47fb-bee3-6bcf7d3339e2",
            name: "First issue",
            sequence_id: 1,
            project_identifier: "CT1",
            project_id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            workspace_slug: "ws-x",
        };
        let rendered_map = render_issue_search(&row);
        assert_eq!(map_keys(&rendered_map), SEARCH_FIELDS);
        assert_eq!(
            rendered(&rendered_map),
            serde_json::to_string(&golden["render"]).expect("serializes"),
        );
    }

    #[test]
    fn search_validate_missing_matches_golden() {
        let fx = fixture();
        let golden = unit(&fx, "IssueSearchSerializer");
        // The probe input carried only `id` (five `required` failures).
        let mut input = Map::new();
        input.insert(
            "id".to_string(),
            Value::String("a7509d00-345f-47fb-bee3-6bcf7d3339e2".to_string()),
        );
        let errors = validate_issue_search(&input).expect_err("five fields missing");
        assert_eq!(errors.len(), 5);
        // Structured (message, code) pairs match the golden's `errors` map.
        for error in &errors {
            let entry = golden["missing_field"]["errors"][error.field]
                .as_array()
                .unwrap_or_else(|| panic!("golden lacks errors.{}", error.field));
            assert_eq!(entry.len(), 1);
            assert_eq!(
                entry[0]["message"],
                Value::String(error.message.to_string())
            );
            assert_eq!(entry[0]["code"], Value::String(error.code.to_string()));
        }
        assert_eq!(golden["missing_field"]["valid"], Value::Bool(false));
        // The wire body: `{field: [message]}` in field order.
        assert_eq!(
            search_errors_body(&errors),
            "{\"name\":[\"This field is required.\"],\
             \"sequence_id\":[\"This field is required.\"],\
             \"project__identifier\":[\"This field is required.\"],\
             \"project_id\":[\"This field is required.\"],\
             \"workspace__slug\":[\"This field is required.\"]}",
        );
    }

    #[test]
    fn search_validate_charfield_rules() {
        let full = |value: Value| {
            let mut input = Map::new();
            for field in SEARCH_FIELDS {
                input.insert(field.to_string(), value.clone());
            }
            input
        };
        // Numerics coerce via `str()`.
        let validated = validate_issue_search(&full(json!(7))).expect("numbers coerce");
        assert_eq!(validated.sequence_id, "7");
        let validated = validate_issue_search(&full(json!(1.5))).expect("floats coerce");
        assert_eq!(validated.sequence_id, "1.5");
        // Strings trim.
        let validated = validate_issue_search(&full(json!("  padded  "))).expect("trims");
        assert_eq!(validated.name, "padded");
        // Bools, nulls, composites, blanks fail with DRF's codes.
        let errors = validate_issue_search(&full(Value::Bool(true))).expect_err("bools fail");
        assert!(errors.iter().all(|e| e.code == "invalid"));
        let errors = validate_issue_search(&full(Value::Null)).expect_err("nulls fail");
        assert!(errors.iter().all(|e| e.code == "null"));
        let errors = validate_issue_search(&full(json!(["x"]))).expect_err("lists fail");
        assert!(errors.iter().all(|e| e.message == "Not a valid string."));
        let errors = validate_issue_search(&full(json!(""))).expect_err("blanks fail");
        assert!(errors.iter().all(|e| e.code == "blank"));
        let errors = validate_issue_search(&full(json!("   "))).expect_err("whitespace fails");
        assert!(errors.iter().all(|e| e.code == "blank"));
        let errors = validate_issue_search(&full(json!("a\x00b"))).expect_err("null chars fail");
        assert!(errors
            .iter()
            .all(|e| e.code == "null_characters_not_allowed"));
        // Unknown keys are ignored.
        let mut input = full(json!("v"));
        input.insert("unknown".to_string(), json!(1));
        assert!(validate_issue_search(&input).is_ok());
    }

    fn advanced_row() -> AdvancedSearchResultRow<'static> {
        AdvancedSearchResultRow {
            id: "a7509d00-345f-47fb-bee3-6bcf7d3339e2",
            sequence_id: 1,
            identifier: "CT1-1",
            name: "First issue",
            snippet: Some("around the match"),
            state: Some(AdvancedSearchStateView {
                name: Some("Todo"),
                group: Some("unstarted"),
            }),
            project: AdvancedSearchProjectView {
                id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
                identifier: "CT1",
                name: "P",
            },
            workspace_slug: "ws-x",
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-02T00:00:00Z",
            completed_at: None,
            rank: 0.75,
            url: UrlPresence::Value("http://x/ws-x/browse/CT1-1"),
        }
    }

    #[test]
    fn advanced_result_renders_match_golden_byte_identical() {
        let fx = fixture();
        let golden = unit(&fx, "IssueAdvancedSearchResultSerializer");
        let rendered_map = render_advanced_result(&advanced_row()).expect("renders");
        assert_eq!(map_keys(&rendered_map), ADVANCED_RESULT_FIELDS);
        assert_eq!(
            rendered(&rendered_map),
            serde_json::to_string(&golden["render"]).expect("serializes"),
        );

        // Explicit-`None` url renders a present `null` key.
        let mut no_url = advanced_row();
        no_url.url = UrlPresence::Null;
        let rendered_map = render_advanced_result(&no_url).expect("renders");
        assert_eq!(
            rendered(&rendered_map),
            serde_json::to_string(&golden["render_no_url"]).expect("serializes"),
        );

        // Absent url omits the key (the wire case); null state/snippet render
        // `null` keys.
        let mut bare = advanced_row();
        bare.url = UrlPresence::Absent;
        bare.state = None;
        bare.snippet = None;
        let rendered_map = render_advanced_result(&bare).expect("renders");
        assert!(!rendered_map.contains_key("url"));
        assert_eq!(rendered_map["state"], Value::Null);
        assert_eq!(golden["null_state"], Value::Null);
        assert_eq!(rendered_map["snippet"], Value::Null);
        assert_eq!(rendered_map["completed_at"], Value::Null);

        // Stateless wire rows render `{name: null, group: null}`, never a
        // null `state`.
        let mut stateless = advanced_row();
        stateless.state = Some(AdvancedSearchStateView {
            name: None,
            group: None,
        });
        let rendered_map = render_advanced_result(&stateless).expect("renders");
        assert_eq!(rendered_map["state"], json!({"name": null, "group": null}));
    }

    #[test]
    fn advanced_response_render_matches_golden_byte_identical() {
        let fx = fixture();
        let golden = unit(&fx, "IssueAdvancedSearchResponseSerializer");
        let result = render_advanced_result(&advanced_row()).expect("renders");
        let response = render_advanced_response("match", 1, vec![result]);
        assert_eq!(map_keys(&response), vec!["query", "count", "results"]);
        assert_eq!(
            rendered(&response),
            serde_json::to_string(&golden["render"]).expect("serializes"),
        );
    }

    #[test]
    fn advanced_rank_rejects_non_finite() {
        let mut row = advanced_row();
        row.rank = f64::INFINITY;
        assert_eq!(
            render_advanced_result(&row),
            Err(ExpandSearchError::NonFiniteFloat("rank")),
        );
    }
}
