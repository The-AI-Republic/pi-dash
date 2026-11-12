//! Cycle serializer shapes: pure field lists, validation rules, read views (D-20, stage 5).
//!
//! Port of `apps/api/pi_dash/api/serializers/cycle.py:1-206`
//! (drift baseline `01a93e17`; zero diff Ported-from..HEAD re-verified
//! 2026-09-30):
//!
//! * `CycleCreateSerializer` (`:15-106`) → [`CREATE_FIELDS`] +
//!   [`CREATE_READ_ONLY_FIELDS`] + `validate` (`:61-106`): the project-id
//!   resolution (`:67-75`), the three project gates (`:77-84`), the
//!   date-ordering rule (`:85-90`), the `convert_to_utc` rewrite (`:92-101`)
//!   and the `owned_by` default (`:103-104`). The `__init__` project-timezone
//!   wiring (`:30-36`) is [`INPUT_TZ_FIELDS`].
//! * `CycleUpdateSerializer` (`:109-121`) → same 8 fields: its `Meta.fields`
//!   re-appends the already-present `owned_by` (BUG-update-dup), which DRF
//!   dedupes — [`UPDATE_FIELDS`] is identical to [`CREATE_FIELDS`] by
//!   construction. It inherits `validate` unchanged.
//! * `CycleSerializer` (`:124-155`) → [`CYCLE_READ_FIELDS`] (DRF wire order,
//!   probed) + [`CYCLE_METRIC_FIELDS`] + [`CycleReadView`]. The 9 declared
//!   metric fields render only when the queryset carries the annotation;
//!   a missing annotation drops the key (`SkipField`), never `null`
//!   ([`metric_present`]).
//! * `CycleIssueSerializer` (`:158-171`) → [`CYCLE_ISSUE_READ_FIELDS`] +
//!   [`CycleIssueReadView`]; `sub_issues_count` follows the same
//!   annotation rule (annotated in `api/views/cycle.py:815-823`).
//! * `CycleLiteSerializer` (`:174-184`) → [`CYCLE_LITE_FIELDS`] +
//!   [`CycleLiteView`]: `__all__` with no declared extras and no
//!   `read_only_fields`, so everything but `id` is writable.
//! * `CycleIssueRequestSerializer` (`:187-195`): [`validate_issues_field`]
//!   plus DRF-native bodies. Docs-only on the wire (BUG-docs-only): the
//!   add-issues view (`api/views/cycle.py:920-935`) reads `request.data`
//!   manually and answers its own `MISSING_WORK_ITEMS` / `CYCLE_COMPLETED`
//!   bodies before any serializer runs.
//! * `TransferCycleIssueRequestSerializer` (`:198-206`) →
//!   [`validate_transfer_field`] + [`transfer_required_body`] /
//!   [`transfer_null_body`] / [`transfer_uuid_error_body`]. Docs-only on the
//!   wire (BUG-docs-only): the transfer view (`api/views/cycle.py:1167-1174`)
//!   answers `{"error": "New Cycle Id is required"}` itself on a falsy
//!   `new_cycle_id`.
//!
//! Fixture oracle: FX-CYCMOD-02
//! (`rust-api/fixtures/v1_cycles_modules/serializers/cycle.golden.json` +
//! `TRACE.md`); the unit tests below assert the field lists, read-only sets,
//! declared fields and `raises` messages against that file so transcription
//! drift fails the build. DRF wire order, per-field attrs and request error
//! bodies were verified against live DRF (Django 4.2.30, `test` settings,
//! no DB) with a field-introspection probe on 2026-09-30; the probe values
//! are quoted in the tests.
//!
//! Layering notes (D-21 types precedent):
//!
//! * Datetimes cross this boundary already rendered as DRF iso-8601 strings
//!   and ids as strings (borrowed `&str`); rendering owns to the handlers
//!   layer ([`render_drf_datetime_utc`]). The DB reads inside `validate`
//!   (the `Project` lookup at `:80`, the project timezone inside
//!   `convert_to_utc`) live outside the types crate (crate graph
//!   `types -> db -> services -> api`), so the pure rules take their
//!   outcomes as arguments; the services sibling (`services/src/
//!   v1_cycles_modules/cycle_shapes.rs`) composes them into the full
//!   `validate()` flow.
//! * `None`-vs-absent-key trap: DRF folds both to `None` in `validated_data`
//!   (`data.get("start_date", None)`), so one `Option` per date covers both
//!   — except where field-level validation distinguishes them (name,
//!   description, timezone take `Option<Option<&str>>`: absent vs explicit
//!   `null` fail differently).
//! * No `[:255]`-style truncation exists in this serializer or its views
//!   (grep-verified 2026-09-30); over-long input is rejected by
//!   `MaxLengthValidator`, never sliced. Length is counted in Unicode scalar
//!   values (`len(value)`), never bytes.
//!
//! Ported bugs (translate, don't redesign):
//!
//! 1. BUG-update-dup (`cycle.py:117-122`): `CycleUpdateSerializer.Meta.fields`
//!    is `Create.fields + ["owned_by"]` although `owned_by` is already entry
//!    5 of `Create.fields` (`:45`). Harmless — DRF dedupes — and the
//!    completed-cycle sort_order PATCH stays a 200 no-op because the narrowed
//!    `{"sort_order": …}` payload matches no field here
//!    (`api/views/cycle.py:512-515`; contract `test_patch_completed_cycle_
//!    sort_order_silently_dropped`).
//! 2. BUG-docs-only (`cycle.py:187-206` + views above): both request
//!    serializers are referenced only as OpenAPI request schemas; the views
//!    validate `request.data` by hand with different envelopes.
//! 3. BUG-create-read-only (`cycle.py:50-59`): `CycleCreateSerializer.Meta.
//!    read_only_fields` lists 8 fields, none of which is in `Meta.fields` —
//!    DRF ignores unknown read-only entries, so the list constrains nothing.
//! 4. `TIMEZONE_CHOICES` is a class-level constant over
//!    `pytz.common_timezones` (`db/models/cycle.py:78`), not a column; the
//!    membership set is runtime pytz data, so only the count observed at
//!    probe time (433) is quoted, never pinned.
//! 5. The create-time both-or-neither date rule lives in the view
//!    (`api/views/cycle.py:306-310`: `{"error": "Both start date and end
//!    date are either required or are to be null"}`), not the serializer —
//!    the serializer accepts a lone date and the view rejects it. Only the
//!    serializer half is ported here.
//!
//! Pages read: PIDASHCONV-1 rulebook (updated_at 2026-09-30T05:31:02.403174Z);
//! Porting Guide `4496e321-dd24-40f7-bfdf-f771e45fac0c` (updated_at
//! 2026-09-28T03:51:35.921141Z); Dead Python Code
//! `05399703-0404-49f6-a680-5924d6df7640` (updated_at
//! 2026-09-23T08:30:31.434101Z; no cycle skip rows, disposition `full`).

use serde::ser::SerializeStruct;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// `CycleCreateSerializer.Meta.fields`, source order (`cycle.py:40-49`).
pub const CREATE_FIELDS: [&str; 8] = [
    "name",
    "description",
    "start_date",
    "end_date",
    "owned_by",
    "external_source",
    "external_id",
    "timezone",
];

/// `CycleCreateSerializer.Meta.read_only_fields` (`cycle.py:50-59`).
/// BUG-create-read-only: none of these is in [`CREATE_FIELDS`], so DRF
/// ignores every entry — writes accept exactly the 8 `CREATE_FIELDS`.
/// Recorded for the handlers layer.
pub const CREATE_READ_ONLY_FIELDS: [&str; 8] = [
    "id",
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "deleted_at",
];

/// `CycleUpdateSerializer.Meta.fields` (`cycle.py:117-121`).
/// BUG-update-dup: `Create.fields + ["owned_by"]`, but `owned_by` is already
/// entry 5 — the live field set (probed) is identical to [`CREATE_FIELDS`].
pub const UPDATE_FIELDS: [&str; 8] = CREATE_FIELDS;

/// The only required create/update input: `name` is `CharField` without
/// `required=False` (`cycle.py:61` model field, probed `required=True`).
/// Every other create field is optional.
pub const CREATE_REQUIRED_FIELDS: [&str; 1] = ["name"];

/// `Cycle.name` / `external_source` / `external_id` column caps
/// (`db/models/cycle.py:61,72,73`, probed `max_length=255`).
pub const NAME_MAX_LENGTH: usize = 255;
/// `external_source` / `external_id` share the 255 cap.
pub const EXTERNAL_MAX_LENGTH: usize = 255;

/// `CycleCreateSerializer.__init__` (`cycle.py:30-36`): when the context
/// project has a timezone, the `start_date`/`end_date` `DateTimeField`s parse
/// naive input in that zone instead of the default. Zone resolution belongs
/// to the query layer; the affected fields are named here.
pub const INPUT_TZ_FIELDS: [&str; 2] = ["start_date", "end_date"];

/// The 9 declared metric fields on `CycleSerializer` (`cycle.py:132-140`),
/// all `read_only=True`, in declaration order. They lead the wire order
/// (probed): DRF emits declared fields before model fields.
pub const CYCLE_METRIC_FIELDS: [&str; 9] = [
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "total_estimates",
    "completed_estimates",
    "started_estimates",
];

/// `CycleSerializer` wire order, all 31 keys (probed against live DRF):
/// `id` (`BaseSerializer`, `api/serializers/base.py:17`), the 9 declared
/// metrics, then the `Cycle` model fields — concrete fields first
/// (`created_at`, `updated_at`, `deleted_at`, `name` … `version`), then
/// forward relations in model order (`created_by`, `updated_by`, `project`,
/// `workspace`, `owned_by`).
pub const CYCLE_READ_FIELDS: [&str; 31] = [
    "id",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "total_estimates",
    "completed_estimates",
    "started_estimates",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "start_date",
    "end_date",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "archived_at",
    "logo_props",
    "timezone",
    "version",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "owned_by",
];

/// `CycleSerializer.Meta.read_only_fields` (`cycle.py:145-155`).
pub const CYCLE_READ_ONLY_FIELDS: [&str; 9] = [
    "id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "workspace",
    "project",
    "owned_by",
    "deleted_at",
];

/// `CycleIssueSerializer` wire order, all 11 keys (probed): `id`, the
/// declared `sub_issues_count`, then the `CycleIssue` model fields —
/// concrete (`created_at`, `updated_at`, `deleted_at`) then forward
/// relations in model order (`created_by`, `updated_by`, `project`,
/// `workspace`, `issue`, `cycle`).
pub const CYCLE_ISSUE_READ_FIELDS: [&str; 11] = [
    "id",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
    "cycle",
];

/// `CycleIssueSerializer.Meta.read_only_fields` (`cycle.py:171`).
pub const CYCLE_ISSUE_READ_ONLY_FIELDS: [&str; 3] = ["workspace", "project", "cycle"];

/// `CycleLiteSerializer` wire order, all 22 keys (probed): `id` then the
/// `Cycle` model fields in the same concrete-then-relations order as
/// [`CYCLE_READ_FIELDS`]. No declared extras, no `read_only_fields`.
pub const CYCLE_LITE_FIELDS: [&str; 22] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "start_date",
    "end_date",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "archived_at",
    "logo_props",
    "timezone",
    "version",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "owned_by",
];

/// `BaseSerializer` expand map entries relevant to cycles
/// (`api/serializers/base.py:91-106`): `expand=<field>` replaces the pk
/// with the lite rendering; any other `expand` value falls back to the
/// `{expand}_id` attribute (`:116`).
pub const CYCLE_EXPANDABLE_FIELDS: [&str; 5] = [
    "owned_by",
    "project",
    "workspace",
    "created_by",
    "updated_by",
];

/// `CycleCreateSerializer.validate` project gates (`cycle.py:77-84`),
/// first raise wins.
pub const PROJECT_ID_REQUIRED_MESSAGE: &str = "Project ID is required";
/// `Project.objects.filter(id=…).first()` is empty (`cycle.py:80-82`).
pub const PROJECT_NOT_FOUND_MESSAGE: &str = "Project not found";
/// `project.cycle_view` is falsy (`cycle.py:83-84`).
pub const CYCLES_DISABLED_MESSAGE: &str = "Cycles are not enabled for this project";
/// Both dates non-null and `start_date > end_date` (`cycle.py:85-90`).
pub const START_AFTER_END_MESSAGE: &str = "Start date cannot exceed end date";

/// Raise path of the `validate()` errors: `validate` raises bare-string
/// `serializers.ValidationError`s, which DRF renders under the
/// `non_field_errors` key with status 400 (same envelope as the D-27 app
/// port, verified against live DRF).
pub fn non_field_error_body(message: &str) -> String {
    format!("{{\"non_field_errors\":[\"{message}\"]}}")
}

/// DRF field-level failures (probed verbatim against live DRF).
pub const REQUIRED_MESSAGE: &str = "This field is required.";
/// Explicit `null` where `allow_null=False`.
pub const NULL_MESSAGE: &str = "This field may not be null.";
/// Empty string where `allow_blank=False`.
pub const BLANK_MESSAGE: &str = "This field may not be blank.";
/// `max_length=255` failure.
pub const NAME_MAX_LENGTH_MESSAGE: &str = "Ensure this field has no more than 255 characters.";
/// `ListField` fed a non-list: `Expected a list of items but got type "str".`
pub fn not_a_list_message(got_type: &str) -> String {
    format!("Expected a list of items but got type \"{got_type}\".")
}
/// `UUIDField` child failure (probed verbatim).
pub const INVALID_UUID_MESSAGE: &str = "Must be a valid UUID.";
/// `ChoiceField` failure shape: `"<input>" is not a valid choice.`
pub fn invalid_choice_message(input: &str) -> String {
    format!("\"{input}\" is not a valid choice.")
}

/// `{"issues": ["This field is required."]}` — absent `issues` key
/// (probed: `{}` → `valid=False`).
pub fn issues_required_body() -> String {
    format!("{{\"issues\":[\"{REQUIRED_MESSAGE}\"]}}")
}

/// `{"issues": ["Expected a list of items but got type \"<ty>\"."]}`
/// (probed with `"nope"` → `"str"`).
pub fn issues_not_list_body(got_type: &str) -> String {
    format!("{{\"issues\":[\"{}\"]}}", not_a_list_message(got_type))
}

/// `{"issues": {"<index>": ["Must be a valid UUID."]}}` — per-item UUID
/// failure keyed by list index (probed: index `0` and `1`).
pub fn issue_uuid_error_body(index: usize) -> String {
    format!("{{\"issues\":{{\"{index}\":[\"{INVALID_UUID_MESSAGE}\"]}}}}")
}

/// `{"new_cycle_id": ["This field is required."]}` — absent key (probed).
pub fn transfer_required_body() -> String {
    format!("{{\"new_cycle_id\":[\"{REQUIRED_MESSAGE}\"]}}")
}

/// `{"new_cycle_id": ["This field may not be null."]}` — explicit `null`
/// (probed; `UUIDField` has `allow_null=False` by default).
pub fn transfer_null_body() -> String {
    format!("{{\"new_cycle_id\":[\"{NULL_MESSAGE}\"]}}")
}

/// `{"new_cycle_id": ["Must be a valid UUID."]}` (probed with `"bad"`).
pub fn transfer_uuid_error_body() -> String {
    format!("{{\"new_cycle_id\":[\"{INVALID_UUID_MESSAGE}\"]}}")
}

/// Whether `new_cycle_id` passes `TransferCycleIssueRequestSerializer`
/// field validation: present, non-null, parses as a UUID (probed matrix:
/// `{}` invalid, `"bad"` invalid, `null` invalid, valid UUID valid).
/// Python's `uuid.UUID()` accepts a small superset (e.g. `{braced}` and
/// `urn:` forms); `Uuid::parse_str` accepts hyphenated, simple, braced and
/// urn forms too, so the residual gap is negligible and documented here.
pub fn is_valid_uuid_str(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}

/// `CycleIssueRequestSerializer.issues` (`cycle.py:195`): `ListField` of
/// `UUIDField`, required, `allow_empty=True` (DRF default — probed: `[]`
/// is valid; emptiness is rejected later by the view with
/// `MISSING_WORK_ITEMS`). `present=false` = key absent (`required`
/// failure); `not_list=Some(ty)` = non-list input, where `ty` is the
/// Python `type(value).__name__` (`"str"`, `"int"`, `"dict"`, `"NoneType"`,
/// `"bool"` — probed with `"str"`); `None` = a list, valid here (per-item
/// UUID failures are [`issue_uuid_error_body`]).
pub fn validate_issues_field(present: bool, not_list: Option<&str>) -> Result<(), String> {
    if !present {
        return Err(issues_required_body());
    }
    if let Some(got_type) = not_list {
        return Err(issues_not_list_body(got_type));
    }
    Ok(())
}

/// `TransferCycleIssueRequestSerializer.new_cycle_id` (`cycle.py:206`):
/// required `UUIDField`, `allow_null=False` (default).
/// `None` = key absent; `Some(None)` = explicit `null`.
pub fn validate_transfer_field(value: Option<Option<&str>>) -> Result<&str, String> {
    let Some(maybe_id) = value else {
        return Err(transfer_required_body());
    };
    let Some(id) = maybe_id else {
        return Err(transfer_null_body());
    };
    if !is_valid_uuid_str(id) {
        return Err(transfer_uuid_error_body());
    }
    Ok(id)
}

/// `CycleCreateSerializer.name` (`db/models/cycle.py:61`): required
/// `CharField`, `max_length=255`, `allow_blank=False`, `allow_null=False`.
/// `None` = key absent (`required`); `Some(None)` = explicit `null`
/// (`null`); empty = `blank`. Length counts Unicode scalar values, as
/// DRF's `MaxLengthValidator` (`len(value)`) does — never slice the input.
pub fn validate_cycle_name(value: Option<Option<&str>>) -> Result<&str, &'static str> {
    let Some(maybe_name) = value else {
        return Err(REQUIRED_MESSAGE);
    };
    let Some(name) = maybe_name else {
        return Err(NULL_MESSAGE);
    };
    if name.is_empty() {
        return Err(BLANK_MESSAGE);
    }
    if name.chars().count() > NAME_MAX_LENGTH {
        return Err(NAME_MAX_LENGTH_MESSAGE);
    }
    Ok(name)
}

/// `CycleCreateSerializer.description` (`db/models/cycle.py:62`):
/// `required=False` (`blank=True`), `allow_blank=True`, `allow_null=False`
/// (no `null=True`). Absent → skipped (`None`); explicit `null` fails;
/// `""` is valid.
pub fn validate_cycle_description(
    value: Option<Option<&str>>,
) -> Result<Option<&str>, &'static str> {
    match value {
        None => Ok(None),
        Some(None) => Err(NULL_MESSAGE),
        Some(Some(description)) => Ok(Some(description)),
    }
}

/// `CycleCreateSerializer.timezone` (`db/models/cycle.py:79`): `ChoiceField`
/// over `TIMEZONE_CHOICES` with model default `"UTC"`, so `required=False`.
/// Absent → the `"UTC"` default; explicit `null` fails (`allow_null=False`
/// default); anything outside the pytz set fails with `invalid_choice`.
/// `is_known` is the caller's membership answer against `pytz.
/// common_timezones` (runtime data — the handler layer owns the set).
pub fn validate_cycle_timezone(
    value: Option<Option<&str>>,
    is_known: bool,
) -> Result<&str, String> {
    match value {
        None => Ok("UTC"),
        Some(None) => Err(NULL_MESSAGE.to_string()),
        Some(Some(tz)) if is_known => Ok(tz),
        Some(Some(tz)) => Err(invalid_choice_message(tz)),
    }
}

/// Mirrors the `CycleCreateSerializer.validate` ordering check
/// (`cycle.py:85-90`): fires only when BOTH dates are non-null. DRF parses
/// an absent key and an explicit `null` to the same `None` before
/// `validate()` runs (`data.get("start_date", None)`), so one `Option<i64>`
/// per side covers both — no separate absent-vs-null branch. Epoch-second
/// comparison is the same ordering as the Python datetime comparison.
///
/// Returns `Err(START_AFTER_END_MESSAGE)` when `start > end`, else `Ok`.
pub fn validate_date_order(
    start_epoch_secs: Option<i64>,
    end_epoch_secs: Option<i64>,
) -> Result<(), &'static str> {
    if let (Some(start), Some(end)) = (start_epoch_secs, end_epoch_secs) {
        if start > end {
            return Err(START_AFTER_END_MESSAGE);
        }
    }
    Ok(())
}

/// Mirrors the `project_id` resolution in `validate()` (`cycle.py:67-75`):
/// `context["project_id"]` wins, then `initial_data.project_id` (the legacy
/// body path, `:69`), then the instance's — first non-null wins. `None`
/// means the `PROJECT_ID_REQUIRED_MESSAGE` arm fires.
pub fn resolve_create_project_id<'a>(
    context_project_id: Option<&'a str>,
    initial_data_project_id: Option<&'a str>,
    instance_project_id: Option<&'a str>,
) -> Option<&'a str> {
    context_project_id
        .or(initial_data_project_id)
        .or(instance_project_id)
}

/// Mirrors the `validate()` project gates in raise order (`cycle.py:77-84`):
/// missing id → unknown id → `cycle_view` off. `project` carries the DB
/// lookup outcome: `None` = no row, `Some(cycle_view_enabled)`.
pub fn validate_project_gate(
    project_id: Option<&str>,
    project: Option<bool>,
) -> Result<&str, &'static str> {
    let Some(id) = project_id else {
        return Err(PROJECT_ID_REQUIRED_MESSAGE);
    };
    let Some(cycle_view) = project else {
        return Err(PROJECT_NOT_FOUND_MESSAGE);
    };
    if !cycle_view {
        return Err(CYCLES_DISABLED_MESSAGE);
    }
    Ok(id)
}

/// Mirrors `if not data.get("owned_by"): data["owned_by"] = request.user`
/// (`cycle.py:103-104`): any provided owner (non-null FK) wins, otherwise
/// the requester becomes the owner.
pub fn owned_by_or_requester<'a>(owned_by: Option<&'a str>, requester: &'a str) -> &'a str {
    owned_by.unwrap_or(requester)
}

/// Mirrors `convert_to_utc` for a cycle start date
/// (`utils/timezone_converter.py:66-85`): local midnight (`time.min`) plus
/// one second, shifted to UTC.
///
/// `utc_offset_secs` is the project's zone offset east of UTC on that date
/// (from `project.timezone` via the IANA database; zone resolution belongs
/// to the query layer). The same-day special case (`:82-83` — when the date
/// is today in the project zone the function returns the current instant in
/// UTC instead) is applied by the caller through
/// `is_today_in_project_tz` + `now_utc_epoch_secs`, mirroring the branch
/// exactly.
///
/// `local_midnight_epoch_secs` is the date at local 00:00:00 expressed as a
/// UTC epoch; callers computing it from a `%Y-%m-%d` string use the
/// `strptime(date, "%Y-%m-%d")` parse (`:61`).
pub fn convert_start_to_utc(
    local_midnight_epoch_secs: i64,
    is_today_in_project_tz: bool,
    now_utc_epoch_secs: i64,
) -> i64 {
    if is_today_in_project_tz {
        return now_utc_epoch_secs;
    }
    local_midnight_epoch_secs + 1
}

/// Mirrors `convert_to_utc` for a cycle end date
/// (`utils/timezone_converter.py:86-94`): local 23:59:00 shifted to UTC.
/// There is no same-day special case on this branch.
pub fn convert_end_to_utc(local_midnight_epoch_secs: i64) -> i64 {
    local_midnight_epoch_secs + 23 * 3600 + 59 * 60
}

/// Mirrors the `convert_to_utc` input guard (`:57-58`): a missing date or a
/// missing project timezone raises `ValueError` (an uncaught 500 in the
/// `validate()` path — ported, not softened).
pub fn rewrite_inputs_present(date: Option<&str>, project_timezone: Option<&str>) -> bool {
    date.is_some_and(|d| !d.is_empty()) && project_timezone.is_some_and(|tz| !tz.is_empty())
}

/// Whether a declared metric key is rendered: a missing queryset annotation
/// drops the key (`SkipField` — contract `CREATE_KEYS` has no `total_*`
/// keys on create because the view re-serializes without `get_queryset()`),
/// while a present-but-`None` annotation (e.g. `SUM` over no rows) renders
/// `null` with the key present. `Some(None)` = present null.
pub fn metric_present<T>(annotation: Option<Option<T>>) -> bool {
    annotation.is_some()
}

/// Renders an epoch instant the way DRF renders datetimes: ISO-8601 with a
/// `Z` suffix for UTC (`DateTimeField.to_representation`, verified against
/// live DRF). Cycle dates are stored in UTC, so the wire form is always the
/// `Z` branch. Semantic-trap note: this renders the stored instant — zone
/// shifting of `created_at`/`updated_at` into the actor's zone
/// (`user_timezone_converter`) is a read-path rule owned by the queries
/// layer.
pub fn render_drf_datetime_utc(epoch_secs: i64) -> String {
    const BASE: &str = "1970-01-01T00:00:00Z";
    if epoch_secs == 0 {
        return BASE.to_string();
    }
    // Civil-from-days (Howard Hinnant's algorithm): pure integer math, no
    // timezone database — the instant is already UTC.
    let days = epoch_secs.div_euclid(86_400);
    let secs_of_day = epoch_secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    y += i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// A metric annotation value: `None` = annotation missing (key omitted);
/// `Some(None)` = present but SQL `NULL` (key present, `null`);
/// `Some(Some(v))` = rendered value.
pub type Metric<T> = Option<Option<T>>;

/// `CycleSerializer` output shape (`cycle.py:124-155`).
///
/// Wire order is [`CYCLE_READ_FIELDS`]. The 9 metrics are annotation-fed
/// (key omitted when the annotation is missing); every other key is always
/// present. Plain FKs render as pk strings (`PrimaryKeyRelatedField`,
/// read-only); a null FK renders `null`. Datetimes cross this boundary
/// already rendered as DRF iso-8601 strings.
pub struct CycleReadView<'a> {
    /// `BaseSerializer.id` (`api/serializers/base.py:17`).
    pub id: &'a str,
    pub total_issues: Metric<i64>,
    pub cancelled_issues: Metric<i64>,
    pub completed_issues: Metric<i64>,
    pub started_issues: Metric<i64>,
    pub unstarted_issues: Metric<i64>,
    pub backlog_issues: Metric<i64>,
    pub total_estimates: Metric<f64>,
    pub completed_estimates: Metric<f64>,
    pub started_estimates: Metric<f64>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub view_props: Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub progress_snapshot: Value,
    pub archived_at: Option<&'a str>,
    pub logo_props: Value,
    pub timezone: &'a str,
    pub version: i64,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub owned_by: &'a str,
}

fn opt_metric<S, T>(
    out: &mut <S as serde::Serializer>::SerializeStruct,
    key: &'static str,
    metric: Metric<T>,
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

fn opt_string<S>(
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

impl Serialize for CycleReadView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("CycleReadView", 31)?;
        out.serialize_field("id", self.id)?;
        opt_metric::<S, i64>(&mut out, "total_issues", self.total_issues)?;
        opt_metric::<S, i64>(&mut out, "cancelled_issues", self.cancelled_issues)?;
        opt_metric::<S, i64>(&mut out, "completed_issues", self.completed_issues)?;
        opt_metric::<S, i64>(&mut out, "started_issues", self.started_issues)?;
        opt_metric::<S, i64>(&mut out, "unstarted_issues", self.unstarted_issues)?;
        opt_metric::<S, i64>(&mut out, "backlog_issues", self.backlog_issues)?;
        opt_metric::<S, f64>(&mut out, "total_estimates", self.total_estimates)?;
        opt_metric::<S, f64>(&mut out, "completed_estimates", self.completed_estimates)?;
        opt_metric::<S, f64>(&mut out, "started_estimates", self.started_estimates)?;
        out.serialize_field("created_at", self.created_at)?;
        out.serialize_field("updated_at", self.updated_at)?;
        opt_string::<S>(&mut out, "deleted_at", self.deleted_at)?;
        out.serialize_field("name", self.name)?;
        out.serialize_field("description", self.description)?;
        opt_string::<S>(&mut out, "start_date", self.start_date)?;
        opt_string::<S>(&mut out, "end_date", self.end_date)?;
        out.serialize_field("view_props", &self.view_props)?;
        out.serialize_field("sort_order", &self.sort_order)?;
        opt_string::<S>(&mut out, "external_source", self.external_source)?;
        opt_string::<S>(&mut out, "external_id", self.external_id)?;
        out.serialize_field("progress_snapshot", &self.progress_snapshot)?;
        opt_string::<S>(&mut out, "archived_at", self.archived_at)?;
        out.serialize_field("logo_props", &self.logo_props)?;
        out.serialize_field("timezone", self.timezone)?;
        out.serialize_field("version", &self.version)?;
        opt_string::<S>(&mut out, "created_by", self.created_by)?;
        opt_string::<S>(&mut out, "updated_by", self.updated_by)?;
        out.serialize_field("project", self.project)?;
        out.serialize_field("workspace", self.workspace)?;
        out.serialize_field("owned_by", self.owned_by)?;
        out.end()
    }
}

/// `CycleIssueSerializer` output shape (`cycle.py:158-171`).
///
/// Wire order is [`CYCLE_ISSUE_READ_FIELDS`]. `sub_issues_count` is
/// annotation-fed (key omitted when the annotation is missing). `issue`
/// stays writable in Python (not in `read_only_fields`); `project`,
/// `workspace` and `cycle` are read-only. Datetimes are pre-rendered DRF
/// strings; ids are strings.
pub struct CycleIssueReadView<'a> {
    /// `BaseSerializer.id` (`api/serializers/base.py:17`).
    pub id: &'a str,
    /// Annotated sub-issue count (`api/views/cycle.py:815-823`).
    pub sub_issues_count: Metric<i64>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub cycle: &'a str,
}

impl Serialize for CycleIssueReadView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("CycleIssueReadView", 11)?;
        out.serialize_field("id", self.id)?;
        opt_metric::<S, i64>(&mut out, "sub_issues_count", self.sub_issues_count)?;
        out.serialize_field("created_at", self.created_at)?;
        out.serialize_field("updated_at", self.updated_at)?;
        opt_string::<S>(&mut out, "deleted_at", self.deleted_at)?;
        opt_string::<S>(&mut out, "created_by", self.created_by)?;
        opt_string::<S>(&mut out, "updated_by", self.updated_by)?;
        out.serialize_field("project", self.project)?;
        out.serialize_field("workspace", self.workspace)?;
        out.serialize_field("issue", self.issue)?;
        out.serialize_field("cycle", self.cycle)?;
        out.end()
    }
}

/// `CycleLiteSerializer` output shape (`cycle.py:174-184`).
///
/// Wire order is [`CYCLE_LITE_FIELDS`]: `id` then the `Cycle` model fields
/// in the same concrete-then-relations order as [`CYCLE_READ_FIELDS`], with
/// no metric keys. Everything but `id` is writable in Python.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CycleLiteView<'a> {
    /// `BaseSerializer.id` (`api/serializers/base.py:17`).
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub view_props: Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub progress_snapshot: Value,
    pub archived_at: Option<&'a str>,
    pub logo_props: Value,
    pub timezone: &'a str,
    pub version: i64,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub owned_by: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/v1_cycles_modules/serializers/cycle.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("FX-CYCMOD-02 golden exists"))
            .expect("golden parses")
    }

    fn str_list(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("golden carries a list")
            .iter()
            .map(|v| v.as_str().expect("field names are strings"))
            .collect()
    }

    /// Top-level JSON key order of a serialization (serde structs emit in
    /// declaration order; `preserve_order` keeps it in the `Value`).
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        serde_json::to_value(value)
            .expect("serializes")
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn create_fields_and_guards_match_golden() {
        let unit = &fixture()["serializers"]["CycleCreateSerializer"];
        assert_eq!(str_list(&unit["meta"]["fields"]), CREATE_FIELDS);
        assert_eq!(
            str_list(&unit["meta"]["read_only_fields"]),
            CREATE_READ_ONLY_FIELDS
        );
        assert_eq!(unit["meta"]["model"], "Name(id='Cycle', ctx=Load())");
        // The one declared field is the writable `owned_by` override.
        let declared = unit["declared_fields"].as_array().expect("declared fields");
        assert_eq!(declared.len(), 1);
        assert_eq!(declared[0]["name"], "owned_by");
        assert_eq!(declared[0]["line"], 23);
        // `__init__` (project tz wiring) + `validate` spans.
        let methods: Vec<&str> = unit["methods"]
            .as_array()
            .expect("methods")
            .iter()
            .map(|m| m["name"].as_str().expect("method name"))
            .collect();
        assert_eq!(methods, ["__init__", "validate"]);
    }

    #[test]
    fn create_raises_match_golden_in_order() {
        let raises = fixture()["serializers"]["CycleCreateSerializer"]["raises"]
            .as_array()
            .expect("raises")
            .iter()
            .map(|r| {
                (
                    r["line"].as_u64().expect("line"),
                    r["message"].as_str().expect("message").to_string(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            raises,
            [
                (78, PROJECT_ID_REQUIRED_MESSAGE.to_string()),
                (82, PROJECT_NOT_FOUND_MESSAGE.to_string()),
                (84, CYCLES_DISABLED_MESSAGE.to_string()),
                (90, START_AFTER_END_MESSAGE.to_string()),
            ]
        );
        // Each raise renders under `non_field_errors` (bare-string raise).
        assert_eq!(
            non_field_error_body(START_AFTER_END_MESSAGE),
            r#"{"non_field_errors":["Start date cannot exceed end date"]}"#
        );
    }

    #[test]
    fn update_resolves_to_create_fields() {
        // BUG-update-dup: `Meta.fields` is a BinOp (`Create.fields +
        // ["owned_by"]`), recorded verbatim in the golden — the live field
        // set (probed) dedupes to the same 8 in the same order.
        let unit = &fixture()["serializers"]["CycleUpdateSerializer"];
        assert!(
            unit["meta"]["fields"]
                .as_str()
                .expect("BinOp")
                .contains("owned_by"),
            "golden records the duplicate append"
        );
        assert!(unit["declared_fields"]
            .as_array()
            .expect("declared")
            .is_empty());
        assert!(unit["methods"].as_array().expect("methods").is_empty());
        assert_eq!(UPDATE_FIELDS, CREATE_FIELDS);
        assert_eq!(UPDATE_FIELDS.len(), 8);
    }

    #[test]
    fn read_metrics_and_guards_match_golden() {
        let unit = &fixture()["serializers"]["CycleSerializer"];
        assert_eq!(unit["meta"]["fields"], "__all__");
        let declared: Vec<&str> = unit["declared_fields"]
            .as_array()
            .expect("declared")
            .iter()
            .map(|f| f["name"].as_str().expect("field name"))
            .collect();
        assert_eq!(declared, CYCLE_METRIC_FIELDS);
        assert_eq!(
            str_list(&unit["meta"]["read_only_fields"]),
            CYCLE_READ_ONLY_FIELDS
        );
    }

    #[test]
    fn issue_and_lite_match_golden() {
        let fixture = fixture();
        let issue = &fixture["serializers"]["CycleIssueSerializer"];
        assert_eq!(issue["meta"]["fields"], "__all__");
        let declared: Vec<&str> = issue["declared_fields"]
            .as_array()
            .expect("declared")
            .iter()
            .map(|f| f["name"].as_str().expect("field name"))
            .collect();
        assert_eq!(declared, ["sub_issues_count"]);
        assert_eq!(
            str_list(&issue["meta"]["read_only_fields"]),
            CYCLE_ISSUE_READ_ONLY_FIELDS
        );

        let lite = &fixture["serializers"]["CycleLiteSerializer"];
        assert_eq!(lite["meta"]["fields"], "__all__");
        assert!(lite["declared_fields"]
            .as_array()
            .expect("declared")
            .is_empty());

        for name in [
            "CycleIssueRequestSerializer",
            "TransferCycleIssueRequestSerializer",
        ] {
            let unit = &fixture["serializers"][name];
            assert!(unit["methods"].as_array().expect("methods").is_empty());
            assert!(unit["raises"].as_array().expect("raises").is_empty());
        }
        let declared: Vec<&str> = fixture["serializers"]["CycleIssueRequestSerializer"]
            ["declared_fields"]
            .as_array()
            .expect("declared")
            .iter()
            .map(|f| f["name"].as_str().expect("field name"))
            .collect();
        assert_eq!(declared, ["issues"]);
        let declared: Vec<&str> = fixture["serializers"]["TransferCycleIssueRequestSerializer"]
            ["declared_fields"]
            .as_array()
            .expect("declared")
            .iter()
            .map(|f| f["name"].as_str().expect("field name"))
            .collect();
        assert_eq!(declared, ["new_cycle_id"]);
    }

    fn full_read_view() -> CycleReadView<'static> {
        CycleReadView {
            id: "11111111-1111-4111-8111-111111111111",
            total_issues: Some(Some(2)),
            cancelled_issues: Some(Some(0)),
            completed_issues: Some(Some(1)),
            started_issues: Some(Some(1)),
            unstarted_issues: Some(Some(0)),
            backlog_issues: Some(Some(0)),
            total_estimates: Some(Some(8.0)),
            completed_estimates: Some(Some(3.0)),
            started_estimates: Some(Some(5.0)),
            created_at: "2026-09-29T00:00:00Z",
            updated_at: "2026-09-29T00:00:01Z",
            deleted_at: None,
            name: "CT active cycle",
            description: "",
            start_date: Some("2026-10-01T00:00:00Z"),
            end_date: Some("2026-10-08T00:00:00Z"),
            view_props: json!({}),
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            progress_snapshot: json!({}),
            archived_at: None,
            logo_props: json!({}),
            timezone: "UTC",
            version: 1,
            created_by: Some("44444444-4444-4444-8444-444444444444"),
            updated_by: None,
            project: "66666666-6666-4666-8666-666666666666",
            workspace: "55555555-5555-4555-8555-555555555555",
            owned_by: "44444444-4444-4444-8444-444444444444",
        }
    }

    #[test]
    fn read_view_key_order_matches_probed_wire_order() {
        assert_eq!(serialized_keys(&full_read_view()), CYCLE_READ_FIELDS);
        // Float metrics keep their `.0` (DRF `FloatField`, e.g. 65535.0).
        let body = serde_json::to_value(full_read_view()).expect("value");
        assert_eq!(body["sort_order"], json!(65535.0));
        assert_eq!(body["total_estimates"], json!(8.0));
        assert_eq!(body["start_date"], json!("2026-10-01T00:00:00Z"));
        assert_eq!(body["deleted_at"], Value::Null);
    }

    #[test]
    fn missing_annotations_drop_metric_keys() {
        // Contract CREATE_KEYS: the create response re-serializes without
        // `get_queryset()`, so every metric key is absent (SkipField).
        let mut view = full_read_view();
        view.total_issues = None;
        view.cancelled_issues = None;
        view.completed_issues = None;
        view.started_issues = None;
        view.unstarted_issues = None;
        view.backlog_issues = None;
        view.total_estimates = None;
        view.completed_estimates = None;
        view.started_estimates = None;
        let body = serde_json::to_value(&view).expect("value");
        for key in CYCLE_METRIC_FIELDS {
            assert!(
                !body.as_object().expect("object").contains_key(key),
                "{key}"
            );
        }
        assert!(metric_present(Some(Some(2))));
        assert!(!metric_present::<i64>(None));
    }

    #[test]
    fn present_null_metric_renders_null_with_key() {
        // `SUM` over no rows is SQL NULL: the key stays, the value is null.
        let mut view = full_read_view();
        view.total_estimates = Some(None);
        let body = serde_json::to_value(&view).expect("value");
        assert!(body
            .as_object()
            .expect("object")
            .contains_key("total_estimates"));
        assert_eq!(body["total_estimates"], Value::Null);
        assert!(metric_present::<f64>(Some(None)));
    }

    #[test]
    fn issue_and_lite_view_key_orders_match_probed_wire_order() {
        let issue = CycleIssueReadView {
            id: "11111111-1111-4111-8111-111111111111",
            sub_issues_count: Some(Some(3)),
            created_at: "2026-09-29T00:00:00Z",
            updated_at: "2026-09-29T00:00:01Z",
            deleted_at: None,
            created_by: None,
            updated_by: None,
            project: "66666666-6666-4666-8666-666666666666",
            workspace: "55555555-5555-4555-8555-555555555555",
            issue: "77777777-7777-4777-8777-777777777777",
            cycle: "11111111-1111-4111-8111-111111111111",
        };
        assert_eq!(serialized_keys(&issue), CYCLE_ISSUE_READ_FIELDS);
        // Missing annotation drops `sub_issues_count` (unannotated detail).
        let bare = CycleIssueReadView {
            sub_issues_count: None,
            ..issue
        };
        let body = serde_json::to_value(&bare).expect("value");
        assert!(!body
            .as_object()
            .expect("object")
            .contains_key("sub_issues_count"));

        let lite = CycleLiteView {
            id: "11111111-1111-4111-8111-111111111111",
            created_at: "2026-09-29T00:00:00Z",
            updated_at: "2026-09-29T00:00:01Z",
            deleted_at: None,
            name: "CT draft cycle",
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
            project: "66666666-6666-4666-8666-666666666666",
            workspace: "55555555-5555-4555-8555-555555555555",
            owned_by: "44444444-4444-4444-8444-444444444444",
        };
        assert_eq!(serialized_keys(&lite), CYCLE_LITE_FIELDS);
    }

    #[test]
    fn name_description_timezone_kernels_match_drf() {
        // Probed: `{}` → name required; `""` → blank; 300 chars → max_length.
        assert_eq!(validate_cycle_name(None), Err(REQUIRED_MESSAGE));
        assert_eq!(validate_cycle_name(Some(None)), Err(NULL_MESSAGE));
        assert_eq!(validate_cycle_name(Some(Some(""))), Err(BLANK_MESSAGE));
        assert_eq!(validate_cycle_name(Some(Some("x"))), Ok("x"));
        let long = "x".repeat(256);
        assert_eq!(
            validate_cycle_name(Some(Some(&long))),
            Err(NAME_MAX_LENGTH_MESSAGE)
        );
        // Unicode scalar count, not bytes: 255 × 2-byte chars is fine.
        let edge = "é".repeat(255);
        assert_eq!(validate_cycle_name(Some(Some(&edge))), Ok(edge.as_str()));
        let over = "é".repeat(256);
        assert_eq!(
            validate_cycle_name(Some(Some(&over))),
            Err(NAME_MAX_LENGTH_MESSAGE)
        );
        // Description: absent skipped, null rejected, blank valid.
        assert_eq!(validate_cycle_description(None), Ok(None));
        assert_eq!(validate_cycle_description(Some(None)), Err(NULL_MESSAGE));
        assert_eq!(validate_cycle_description(Some(Some(""))), Ok(Some("")));
        // Timezone: absent → UTC default; null rejected; unknown rejected
        // with the invalid_choice body (probed with "Nope/Zone").
        assert_eq!(validate_cycle_timezone(None, false), Ok("UTC"));
        assert_eq!(
            validate_cycle_timezone(Some(None), true),
            Err(NULL_MESSAGE.to_string())
        );
        assert_eq!(
            validate_cycle_timezone(Some(Some("Asia/Kolkata")), true),
            Ok("Asia/Kolkata")
        );
        assert_eq!(
            validate_cycle_timezone(Some(Some("Nope/Zone")), false),
            Err("\"Nope/Zone\" is not a valid choice.".to_string())
        );
        assert_eq!(
            invalid_choice_message("Nope/Zone"),
            "\"Nope/Zone\" is not a valid choice."
        );
    }

    #[test]
    fn date_order_and_project_resolution_match_validate() {
        // `cycle.py:85-90`: only both-present fires.
        assert_eq!(
            validate_date_order(Some(10), Some(5)),
            Err(START_AFTER_END_MESSAGE)
        );
        assert_eq!(validate_date_order(None, Some(5)), Ok(()));
        assert_eq!(validate_date_order(Some(10), None), Ok(()));
        assert_eq!(validate_date_order(None, None), Ok(()));
        assert_eq!(validate_date_order(Some(5), Some(5)), Ok(()));
        // `cycle.py:67-75`: context → initial_data → instance.
        assert_eq!(
            resolve_create_project_id(Some("a"), Some("b"), Some("c")),
            Some("a")
        );
        assert_eq!(
            resolve_create_project_id(None, Some("b"), Some("c")),
            Some("b")
        );
        assert_eq!(resolve_create_project_id(None, None, Some("c")), Some("c"));
        assert_eq!(resolve_create_project_id(None, None, None), None);
        // Gates in raise order: id → row → cycle_view.
        assert_eq!(
            validate_project_gate(None, None),
            Err(PROJECT_ID_REQUIRED_MESSAGE)
        );
        assert_eq!(
            validate_project_gate(Some("p"), None),
            Err(PROJECT_NOT_FOUND_MESSAGE)
        );
        assert_eq!(
            validate_project_gate(Some("p"), Some(false)),
            Err(CYCLES_DISABLED_MESSAGE)
        );
        assert_eq!(validate_project_gate(Some("p"), Some(true)), Ok("p"));
        // `if not data.get("owned_by")` → requester is the fallback.
        assert_eq!(owned_by_or_requester(Some("o"), "r"), "o");
        assert_eq!(owned_by_or_requester(None, "r"), "r");
    }

    #[test]
    fn request_error_bodies_match_live_drf_byte_for_byte() {
        // Probed verbatim 2026-09-30.
        assert_eq!(
            issues_required_body(),
            r#"{"issues":["This field is required."]}"#
        );
        assert_eq!(
            issues_not_list_body("str"),
            r#"{"issues":["Expected a list of items but got type "str"."]}"#
        );
        assert_eq!(
            issue_uuid_error_body(0),
            r#"{"issues":{"0":["Must be a valid UUID."]}}"#
        );
        assert_eq!(
            issue_uuid_error_body(1),
            r#"{"issues":{"1":["Must be a valid UUID."]}}"#
        );
        assert_eq!(
            transfer_required_body(),
            r#"{"new_cycle_id":["This field is required."]}"#
        );
        assert_eq!(
            transfer_null_body(),
            r#"{"new_cycle_id":["This field may not be null."]}"#
        );
        assert_eq!(
            transfer_uuid_error_body(),
            r#"{"new_cycle_id":["Must be a valid UUID."]}"#
        );
        assert_eq!(
            validate_issues_field(false, None),
            Err(issues_required_body())
        );
        assert_eq!(
            validate_issues_field(true, Some("str")),
            Err(issues_not_list_body("str"))
        );
        assert_eq!(
            validate_issues_field(true, Some("dict")),
            Err(issues_not_list_body("dict"))
        );
        assert_eq!(validate_issues_field(true, None), Ok(()));
        assert_eq!(validate_transfer_field(None), Err(transfer_required_body()));
        assert_eq!(
            validate_transfer_field(Some(None)),
            Err(transfer_null_body())
        );
        assert_eq!(
            validate_transfer_field(Some(Some("bad"))),
            Err(transfer_uuid_error_body())
        );
        assert_eq!(
            validate_transfer_field(Some(Some("99999999-9999-4999-8999-999999999999"))),
            Ok("99999999-9999-4999-8999-999999999999")
        );
        assert!(is_valid_uuid_str("99999999-9999-4999-8999-999999999999"));
        assert!(!is_valid_uuid_str("bad"));
    }

    #[test]
    fn convert_boundaries_and_render_match_timezone_converter() {
        // Project at +05:30 with no DST (e.g. Asia/Kolkata): 2026-03-01.
        let offset = 5 * 3600 + 30 * 60;
        let day_epoch_utc: i64 = 1_772_323_200; // 2026-03-01T00:00:00Z
        let local_midnight = day_epoch_utc - offset;
        assert_eq!(
            convert_start_to_utc(local_midnight, false, 0),
            local_midnight + 1
        );
        assert_eq!(
            render_drf_datetime_utc(convert_start_to_utc(local_midnight, false, 0)),
            "2026-02-28T18:30:01Z"
        );
        assert_eq!(
            render_drf_datetime_utc(convert_end_to_utc(local_midnight)),
            "2026-03-01T18:29:00Z"
        );
        assert_eq!(
            convert_start_to_utc(local_midnight, true, 9_999_999_999),
            9_999_999_999
        );
        assert!(rewrite_inputs_present(
            Some("2026-03-01"),
            Some("Asia/Kolkata")
        ));
        assert!(!rewrite_inputs_present(None, Some("Asia/Kolkata")));
        assert!(!rewrite_inputs_present(Some("2026-03-01"), None));
        assert!(!rewrite_inputs_present(Some(""), Some("Asia/Kolkata")));
        assert_eq!(render_drf_datetime_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            render_drf_datetime_utc(1_772_323_201),
            "2026-03-01T00:00:01Z"
        );
        // Independent oracle (CPython datetime, UTC): negative epochs, leap
        // days, century boundary, far future.
        for (epoch, expected) in [
            (-1, "1969-12-31T23:59:59Z"),
            (-86_400, "1969-12-31T00:00:00Z"),
            (86_400, "1970-01-02T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_582_934_400, "2020-02-29T00:00:00Z"),
            (4_102_444_800, "2100-01-01T00:00:00Z"),
            (2_147_483_647, "2038-01-19T03:14:07Z"),
            (253_402_300_799, "9999-12-31T23:59:59Z"),
        ] {
            assert_eq!(render_drf_datetime_utc(epoch), expected, "epoch {epoch}");
        }
    }
}
