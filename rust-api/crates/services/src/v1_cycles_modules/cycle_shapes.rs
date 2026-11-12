#![forbid(unsafe_code)]

//! Cycle `validate()` orchestration over caller-supplied facts (D-20, stage 5).
//!
//! Ports `CycleCreateSerializer.validate` (`apps/api/pi_dash/api/serializers/
//! cycle.py:61-106`, drift baseline `01a93e17`) as [`run_create_validate`]:
//! the project-id resolution (`:67-75`), the three project gates in raise
//! order (`:77-84`), the date-ordering rule (`:85-90`), the `convert_to_utc`
//! rewrite of both dates (`:92-101`) and the `owned_by` default (`:103-104`).
//!
//! The DB reads (the `Project` lookup at `:80`, the project timezone inside
//! `convert_to_utc`) belong to the queries layer: it loads
//! [`CycleProjectFacts`] and the date inputs, then calls
//! [`run_create_validate`]. Field shapes, messages and the pure boundary
//! math live in the types sibling
//! (`pidash_types::v1_cycles_modules::cycle_shapes`); nothing is
//! re-derived here.
//!
//! `CycleUpdateSerializer` inherits `validate` unchanged (`cycle.py:109-121`,
//! BUG-update-dup), so updates run the same flow with `instance_project_id`
//! set — which is exactly how the legacy body path (`initial_data.
//! project_id`, `:69`) and the update path (`instance.project_id`, `:70-74`)
//! feed the resolution.
//!
//! Fixture oracle: FX-CYCMOD-02 (same golden as the types sibling); the
//! tests below replay the `validate` branch table, not the golden file.

use pidash_types::v1_cycles_modules::cycle_shapes as shapes;

/// Outcome of the caller's `Project` lookup for `validate()`
/// (`cycle.py:80`): `None` = no row (→ `PROJECT_NOT_FOUND_MESSAGE`).
pub struct CycleProjectFacts {
    /// `project.cycle_view` (`cycle.py:83`).
    pub cycle_view: bool,
}

/// One date input to `validate()`: the already-parsed input datetime plus
/// the project-zone facts `convert_to_utc` needs.
/// `parsed_epoch_secs` is the DRF-parsed input instant as a UTC epoch — what
/// the ordering check compares. `local_midnight_epoch_secs` is the input's
/// `%Y-%m-%d` day (`strptime(date, "%Y-%m-%d")`,
/// `utils/timezone_converter.py:61`) at local 00:00:00 expressed as a UTC
/// epoch — what the rewrite consumes (zone resolution belongs to the
/// caller). `is_today_in_project_tz` serves the start-date same-day special
/// case (`timezone_converter.py:82-83`).
pub struct CycleDateInput {
    /// DRF-parsed input instant, UTC epoch (ordering input).
    pub parsed_epoch_secs: i64,
    /// Input day at local 00:00:00 as a UTC epoch (rewrite input).
    pub local_midnight_epoch_secs: i64,
    /// Same-day special case arm (start dates only).
    pub is_today_in_project_tz: bool,
}

/// Inputs to [`run_create_validate`]: every fact `validate()` reads,
/// already loaded by the caller.
pub struct CreateValidateInput<'a> {
    /// `context["project_id"]` — the URL kwarg, always set on the wire.
    pub context_project_id: Option<&'a str>,
    /// `initial_data.project_id` — the legacy body path (`:69`).
    pub initial_data_project_id: Option<&'a str>,
    /// `instance.project_id` — set on updates (`:70-74`).
    pub instance_project_id: Option<&'a str>,
    /// `None` = the `Project` row does not exist.
    pub project: Option<CycleProjectFacts>,
    /// Validated `start_date`/`end_date` (`None` = absent or explicit null —
    /// DRF folds both before `validate()` runs).
    pub start: Option<CycleDateInput>,
    pub end: Option<CycleDateInput>,
    /// Validated `owned_by` (`None` = absent or explicit null).
    pub owned_by: Option<&'a str>,
    /// `request.user` — the `owned_by` fallback (`:103-104`).
    pub requester: &'a str,
    /// Current instant in UTC, for the start-date same-day arm.
    pub now_utc_epoch_secs: i64,
}

/// The validated write payload: what `serializer.save()` persists.
/// `start_utc_epoch_secs` / `end_utc_epoch_secs` are the post-`validate()`
/// instants: the `convert_to_utc` rewrite when both dates are set, else the
/// parsed inputs untouched (`cycle.py:92` — a lone date skips the rewrite,
/// mirroring the D-27 app port; the view's both-or-neither gate rejects lone
/// dates before the serializer runs, so the skip is unreachable on the
/// wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedCreate<'a> {
    /// Resolved project id (first non-null of context → body → instance).
    pub project_id: &'a str,
    /// Post-`validate()` start instant.
    pub start_utc_epoch_secs: Option<i64>,
    /// Post-`validate()` end instant.
    pub end_utc_epoch_secs: Option<i64>,
    /// Provided owner, else the requester.
    pub owned_by: &'a str,
}

/// The `validate()` failure, in raise order (`cycle.py:77-90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateInvalid {
    /// `project_id` missing from context, body and instance (`:77-78`).
    ProjectIdRequired,
    /// No `Project` row for the id (`:80-82`).
    ProjectNotFound,
    /// `project.cycle_view` is falsy (`:83-84`).
    CyclesDisabled,
    /// Both dates set and `start_date > end_date` (`:85-90`).
    StartAfterEnd,
}

impl CreateInvalid {
    /// The bare-string message Python raises (rendered under
    /// `non_field_errors` with status 400).
    pub fn message(self) -> &'static str {
        match self {
            CreateInvalid::ProjectIdRequired => shapes::PROJECT_ID_REQUIRED_MESSAGE,
            CreateInvalid::ProjectNotFound => shapes::PROJECT_NOT_FOUND_MESSAGE,
            CreateInvalid::CyclesDisabled => shapes::CYCLES_DISABLED_MESSAGE,
            CreateInvalid::StartAfterEnd => shapes::START_AFTER_END_MESSAGE,
        }
    }

    /// The exact 400 body for the failure.
    pub fn error_body(self) -> String {
        shapes::non_field_error_body(self.message())
    }
}

/// View-owned input-shape gate for cycle creation
/// (`api/views/cycle.py:306-310`): the view accepts the payload only when
/// both dates are null/absent or both are set, else
/// `{"error": "Both start date and end date are either required or are to
/// be null"}` (status 400; contract `test_create_invalid`). The serializer
/// half accepts a lone date — only this gate rejects it.
pub const CREATE_DATES_SHAPE_MESSAGE: &str =
    "Both start date and end date are either required or are to be null";

/// The both-or-neither rule above, over date presence (`None` = absent or
/// explicit null, as the view's `request.data.get(…, None)` reads).
pub fn create_dates_shape_ok(start_present: bool, end_present: bool) -> bool {
    start_present == end_present
}

/// Runs `CycleCreateSerializer.validate` (`cycle.py:61-106`) over
/// caller-loaded facts, in Python raise order:
///
/// 1. resolve `project_id` (context → body → instance);
/// 2. project gates (missing id → unknown id → `cycle_view` off);
/// 3. date ordering over the parsed instants (both-present only);
/// 4. `convert_to_utc` rewrite of each set date;
/// 5. `owned_by` defaults to the requester.
///
/// Comparison inputs to step 3 are the *parsed* datetimes; the rewrite in
/// step 4 replaces them with project-day-bound UTC instants, so the epoch
/// compared in step 3 is the pre-rewrite instant — exactly as Python
/// compares `data["start_date"]` before overwriting it.
pub fn run_create_validate(
    input: CreateValidateInput<'_>,
) -> Result<ValidatedCreate<'_>, CreateInvalid> {
    let project_id = shapes::resolve_create_project_id(
        input.context_project_id,
        input.initial_data_project_id,
        input.instance_project_id,
    )
    .ok_or(CreateInvalid::ProjectIdRequired)?;
    let project = input.project.ok_or(CreateInvalid::ProjectNotFound)?;
    if !project.cycle_view {
        return Err(CreateInvalid::CyclesDisabled);
    }
    shapes::validate_date_order(
        input.start.as_ref().map(|d| d.parsed_epoch_secs),
        input.end.as_ref().map(|d| d.parsed_epoch_secs),
    )
    .map_err(|_| CreateInvalid::StartAfterEnd)?;
    // `cycle.py:92`: the rewrite fires only when BOTH dates are set; a lone
    // date passes through as parsed.
    let (start_utc_epoch_secs, end_utc_epoch_secs) =
        match (input.start.as_ref(), input.end.as_ref()) {
            (Some(start), Some(end)) => (
                Some(shapes::convert_start_to_utc(
                    start.local_midnight_epoch_secs,
                    start.is_today_in_project_tz,
                    input.now_utc_epoch_secs,
                )),
                Some(shapes::convert_end_to_utc(end.local_midnight_epoch_secs)),
            ),
            (start, end) => (
                start.map(|d| d.parsed_epoch_secs),
                end.map(|d| d.parsed_epoch_secs),
            ),
        };
    Ok(ValidatedCreate {
        project_id,
        start_utc_epoch_secs,
        end_utc_epoch_secs,
        owned_by: shapes::owned_by_or_requester(input.owned_by, input.requester),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc_project() -> CycleProjectFacts {
        CycleProjectFacts { cycle_view: true }
    }

    /// Kolkata (+05:30, no DST) inputs for 2026-03-01 → 2026-03-08.
    fn week_inputs() -> (CycleDateInput, CycleDateInput) {
        let offset = 5 * 3600 + 30 * 60;
        let start_day_utc: i64 = 1_772_323_200; // 2026-03-01T00:00:00Z
        let end_day_utc: i64 = 1_772_928_000; // 2026-03-08T00:00:00Z
        (
            CycleDateInput {
                // Input instants parse before the rewrite; any orderable pair
                // works here — the rewrite outputs are asserted below.
                parsed_epoch_secs: start_day_utc,
                local_midnight_epoch_secs: start_day_utc - offset,
                is_today_in_project_tz: false,
            },
            CycleDateInput {
                parsed_epoch_secs: end_day_utc,
                local_midnight_epoch_secs: end_day_utc - offset,
                is_today_in_project_tz: false,
            },
        )
    }

    fn base_input<'a>(
        start: Option<CycleDateInput>,
        end: Option<CycleDateInput>,
        requester: &'a str,
    ) -> CreateValidateInput<'a> {
        CreateValidateInput {
            context_project_id: Some("proj-1"),
            initial_data_project_id: None,
            instance_project_id: None,
            project: Some(utc_project()),
            start,
            end,
            owned_by: None,
            requester,
            now_utc_epoch_secs: 0,
        }
    }

    #[test]
    fn full_create_validate_rewrites_dates_and_defaults_owner() {
        let (start, end) = week_inputs();
        let out = run_create_validate(base_input(Some(start), Some(end), "user-9"))
            .expect("valid create");
        assert_eq!(out.project_id, "proj-1");
        assert_eq!(out.owned_by, "user-9");
        // Start: local 2026-03-01T00:00:01+05:30 → 2026-02-28T18:30:01Z.
        assert_eq!(
            out.start_utc_epoch_secs,
            Some(1_772_323_200 - (5 * 3600 + 30 * 60) + 1)
        );
        // End: local 2026-03-08T23:59:00+05:30.
        assert_eq!(
            out.end_utc_epoch_secs,
            Some(1_772_928_000 - (5 * 3600 + 30 * 60) + 23 * 3600 + 59 * 60)
        );
    }

    #[test]
    fn gates_fire_in_python_raise_order() {
        let (start, end) = week_inputs();
        // Missing id beats everything.
        let mut input = base_input(None, None, "r");
        input.context_project_id = None;
        assert_eq!(
            run_create_validate(input).unwrap_err(),
            CreateInvalid::ProjectIdRequired
        );
        // Unknown id beats disabled beats date order.
        let mut input = base_input(Some(start), Some(end), "r");
        input.project = None;
        assert_eq!(
            run_create_validate(input).unwrap_err(),
            CreateInvalid::ProjectNotFound
        );
        let (start, end) = week_inputs();
        let mut input = base_input(Some(start), Some(end), "r");
        input.project = Some(CycleProjectFacts { cycle_view: false });
        assert_eq!(
            run_create_validate(input).unwrap_err(),
            CreateInvalid::CyclesDisabled
        );
        // Reversed instants fire last.
        let bad_start = CycleDateInput {
            parsed_epoch_secs: 200,
            local_midnight_epoch_secs: 100,
            is_today_in_project_tz: false,
        };
        let bad_end = CycleDateInput {
            parsed_epoch_secs: 100,
            local_midnight_epoch_secs: 50,
            is_today_in_project_tz: false,
        };
        let input = base_input(Some(bad_start), Some(bad_end), "r");
        assert_eq!(
            run_create_validate(input).unwrap_err(),
            CreateInvalid::StartAfterEnd
        );
        // Lone dates pass the serializer unconverted (`cycle.py:92` — the
        // view's both-or-neither gate owns the rejection).
        let (start, _) = week_inputs();
        let parsed = start.parsed_epoch_secs;
        let out = run_create_validate(base_input(Some(start), None, "r")).expect("lone start");
        assert_eq!(out.start_utc_epoch_secs, Some(parsed));
        assert!(out.end_utc_epoch_secs.is_none());
    }

    #[test]
    fn project_resolution_prefers_context_then_body_then_instance() {
        let (start, end) = week_inputs();
        // Update path: no context id, instance id feeds the resolution.
        let mut input = base_input(Some(start), Some(end), "r");
        input.context_project_id = None;
        input.instance_project_id = Some("proj-7");
        let out = run_create_validate(input).expect("update validate");
        assert_eq!(out.project_id, "proj-7");
        // Legacy body path beats the instance.
        let (start, end) = week_inputs();
        let mut input = base_input(Some(start), Some(end), "r");
        input.context_project_id = None;
        input.initial_data_project_id = Some("proj-body");
        input.instance_project_id = Some("proj-7");
        let out = run_create_validate(input).expect("body path");
        assert_eq!(out.project_id, "proj-body");
        // Provided owner wins over the requester.
        let (start, end) = week_inputs();
        let mut input = base_input(Some(start), Some(end), "requester");
        input.owned_by = Some("owner-2");
        let out = run_create_validate(input).expect("owner kept");
        assert_eq!(out.owned_by, "owner-2");
    }

    #[test]
    fn error_bodies_are_byte_exact() {
        assert_eq!(
            CreateInvalid::ProjectIdRequired.error_body(),
            r#"{"non_field_errors":["Project ID is required"]}"#
        );
        assert_eq!(
            CreateInvalid::ProjectNotFound.error_body(),
            r#"{"non_field_errors":["Project not found"]}"#
        );
        assert_eq!(
            CreateInvalid::CyclesDisabled.error_body(),
            r#"{"non_field_errors":["Cycles are not enabled for this project"]}"#
        );
        assert_eq!(
            CreateInvalid::StartAfterEnd.error_body(),
            r#"{"non_field_errors":["Start date cannot exceed end date"]}"#
        );
    }

    #[test]
    fn create_dates_shape_gate_is_both_or_neither() {
        assert!(create_dates_shape_ok(false, false));
        assert!(create_dates_shape_ok(true, true));
        assert!(!create_dates_shape_ok(true, false));
        assert!(!create_dates_shape_ok(false, true));
        assert_eq!(
            CREATE_DATES_SHAPE_MESSAGE,
            "Both start date and end date are either required or are to be null"
        );
    }
}
