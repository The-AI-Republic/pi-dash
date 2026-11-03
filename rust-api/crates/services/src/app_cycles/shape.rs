#![forbid(unsafe_code)]

//! Cycle serializer shapes (D-27, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/cycle.py:1-106`
//! (drift baseline `01a93e17`):
//! - `CycleWriteSerializer.validate` (`:15-44`): the start/end ordering
//!   error plus the `convert_to_utc` rewrite of both dates.
//! - `CycleSerializer` (`:46-90`): the read-only field list.
//! - `CycleIssueSerializer` (`:92-100`): nested `issue_detail`
//!   (`IssueStateSerializer`, `source="issue"`) + `sub_issues_count`.
//! - `CycleUserPropertiesSerializer` (`:102-106`): read-only guards.
//!
//! Out of scope: `api/serializers/cycle.py` (D-20 owns it).
//!
//! Fixture oracle: F-C27-01
//! (`rust-api/fixtures/app_cycles/serializers.golden.json`); the unit
//! tests below assert the field list and error body against that file's
//! values so transcription drift fails the build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. The source carries 22 fields, not the 24 the issue text says — the
//!    22 are ported (`CYCLE_SERIALIZER_FIELDS`).
//! 2. `validate()` rewrites dates only when BOTH start and end are
//!    non-null (`:23`); a lone date passes through unconverted.
//! 3. The create-time XOR date rule lives in the view
//!    (`app/views/cycle/base.py:272-274`), not the serializer — the
//!    serializer accepts a lone date and the view rejects it. Only the
//!    serializer half is ported here.

use chrono::{DateTime, FixedOffset, TimeZone, Utc};

/// `CycleSerializer.Meta.fields`, in source order
/// (`app/serializers/cycle.py:61-88`).
///
/// 14 model keys then 8 annotated keys. `read_only_fields = fields`, so
/// writes never go through this serializer — they go through
/// `CycleWriteSerializer`.
pub const CYCLE_SERIALIZER_FIELDS: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "start_date",
    "end_date",
    "owned_by_id",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "logo_props",
    "is_favorite",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "status",
];

/// `CycleWriteSerializer.Meta.read_only_fields`
/// (`app/serializers/cycle.py:44`).
pub const CYCLE_WRITE_READ_ONLY_FIELDS: &[&str] =
    &["workspace", "project", "owned_by", "archived_at"];

/// `CycleIssueSerializer.Meta.read_only_fields`
/// (`app/serializers/cycle.py:100`).
pub const CYCLE_ISSUE_READ_ONLY_FIELDS: &[&str] = &["workspace", "project", "cycle"];

/// `CycleIssueSerializer.issue_detail`: nested `IssueStateSerializer`
/// with `source="issue"`, read-only, never written
/// (`app/serializers/cycle.py:93`).
pub const CYCLE_ISSUE_NESTED_FIELD: &str = "issue_detail";
/// The relation `issue_detail` reads from.
pub const CYCLE_ISSUE_NESTED_SOURCE: &str = "issue";

/// `CycleIssueSerializer.sub_issues_count`: read-only annotated count,
/// never written (`app/serializers/cycle.py:94`; annotated in
/// `app/views/cycle/issue.py:55-60`).
pub const CYCLE_ISSUE_COUNT_FIELD: &str = "sub_issues_count";

/// `CycleUserPropertiesSerializer.Meta.read_only_fields`
/// (`app/serializers/cycle.py:106`).
pub const CYCLE_USER_PROPERTIES_READ_ONLY_FIELDS: &[&str] =
    &["workspace", "project", "cycle", "user"];

/// `BaseSerializer` renders `id` as a read-only
/// `PrimaryKeyRelatedField` (`app/serializers/base.py:8-10`).
pub const ID_FIELD: &str = "id";

/// The `CycleWriteSerializer.validate` ordering error
/// (`app/serializers/cycle.py:22`).
pub const START_AFTER_END_MESSAGE: &str = "Start date cannot exceed end date";

/// Raise path of the ordering error: `validate()` raises a bare-string
/// `serializers.ValidationError`, which DRF renders under the
/// `non_field_errors` key with status 400 (verified against live DRF
/// 3.18.1 — `{'non_field_errors': [ErrorDetail(...)]}`).
pub fn date_order_error_body() -> String {
    format!("{{\"non_field_errors\":[\"{START_AFTER_END_MESSAGE}\"]}}")
}

/// Mirrors `CycleWriteSerializer.validate` (`cycle.py:16-23`).
///
/// The ordering check fires only when BOTH dates are non-null. DRF parses
/// an absent key and an explicit `null` to the same `None` before
/// `validate()` runs (`data.get("start_date", None)`), so one
/// `Option<i64>` per side covers both — there is no separate
/// absent-vs-null branch to port. Comparisons are epoch-second
/// comparisons, the same ordering as the Python datetime comparison.
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

/// Mirrors the `project_id` resolution in `validate()` (`cycle.py:24-28`):
/// `initial_data.project_id` wins, then the instance's, then the request
/// context's — first non-null wins.
pub fn resolve_rewrite_project_id(
    initial_data_project_id: Option<i64>,
    instance_project_id: Option<i64>,
    context_project_id: Option<i64>,
) -> Option<i64> {
    initial_data_project_id
        .or(instance_project_id)
        .or(context_project_id)
}

/// Mirrors `convert_to_utc` for a cycle start date
/// (`utils/timezone_converter.py:69-83`): local midnight (`time.min`)
/// plus one second, shifted to UTC.
///
/// `utc_offset_secs` is the project's zone offset east of UTC on that
/// date (from `project.timezone` via the IANA database; zone resolution
/// belongs to the query layer, not this shape). The same-day special
/// case (`:82-83` — when the date is today in the project zone the
/// function returns the current instant in UTC instead) is applied by
/// the caller through `is_today_in_project_tz` + `now_utc_epoch_secs`,
/// mirroring the branch exactly.
///
/// `local_midnight_epoch_secs` is the date at local 00:00:00 expressed
/// as a UTC epoch (i.e. `days_since_epoch * 86400 - utc_offset_secs`);
/// callers computing it from a `%Y-%m-%d` string use the
/// `strptime(date, "%Y-%m-%d")` parse (`:58`).
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
/// (`utils/timezone_converter.py:84-94`): local 23:59:00 shifted to UTC.
/// There is no same-day special case on this branch.
pub fn convert_end_to_utc(local_midnight_epoch_secs: i64) -> i64 {
    local_midnight_epoch_secs + 23 * 3600 + 59 * 60
}

/// Mirrors the `convert_to_utc` input guard (`:53-55`): a missing date or
/// a missing project timezone raises `ValueError` (an uncaught 500 in
/// the `validate()` path — ported, not softened).
pub fn rewrite_inputs_present(date: Option<&str>, project_timezone: Option<&str>) -> bool {
    date.is_some_and(|d| !d.is_empty()) && project_timezone.is_some_and(|tz| !tz.is_empty())
}

/// Renders an epoch instant the way DRF renders datetimes: ISO-8601, and
/// a `+00:00` suffix becomes `Z` (`DateTimeField.to_representation`,
/// verified against live DRF 3.18.1). Cycle dates are stored in UTC, so
/// the wire form is always the `Z` branch. Semantic-trap note: this is a
/// render of the stored instant, not a zone shift — zone shifting of
/// `created_at`/`updated_at` into the actor's zone is a read-path rule
/// owned by the queries layer.
pub fn render_drf_datetime_utc(epoch_secs: i64) -> String {
    let instant: DateTime<Utc> = Utc
        .timestamp_opt(epoch_secs, 0)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().expect("epoch 0 exists"));
    instant.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// ISO-8601 render for a non-UTC offset (the non-`Z` branch of DRF's
/// `to_representation`, e.g. user-zone-shifted timestamps on read
/// paths): `+00:00` still collapses to `Z`.
pub fn render_drf_datetime_with_offset(epoch_secs: i64, offset_east_secs: i32) -> String {
    let zone = FixedOffset::east_opt(offset_east_secs)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero offset exists"));
    let instant = zone
        .timestamp_opt(epoch_secs, 0)
        .single()
        .unwrap_or_else(|| zone.timestamp_opt(0, 0).single().expect("epoch 0 exists"));
    let rendered = instant.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
    if rendered.ends_with("+00:00") {
        format!("{}Z", &rendered[..rendered.len() - 6])
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_GOLDEN: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_cycles/serializers.golden.json"
    );

    #[test]
    fn cycle_serializer_fields_match_golden_in_order() {
        let golden = std::fs::read_to_string(FIXTURE_GOLDEN).expect("F-C27-01 golden exists");
        let parsed: serde_json::Value =
            serde_json::from_str(&golden).expect("F-C27-01 golden is valid JSON");
        let expected: Vec<&str> = parsed["cycle_serializer"]["field_list"]
            .as_array()
            .expect("golden carries field_list")
            .iter()
            .map(|v| v.as_str().expect("field names are strings"))
            .collect();
        assert_eq!(CYCLE_SERIALIZER_FIELDS.len(), 22);
        assert_eq!(CYCLE_SERIALIZER_FIELDS, expected.as_slice());
    }

    #[test]
    fn read_only_guards_match_golden() {
        let golden = std::fs::read_to_string(FIXTURE_GOLDEN).expect("F-C27-01 golden exists");
        let parsed: serde_json::Value =
            serde_json::from_str(&golden).expect("F-C27-01 golden is valid JSON");
        let write_guards: Vec<&str> = parsed["cycle_write_serializer"]["meta"]["read_only_fields"]
            .as_array()
            .expect("golden carries write guards")
            .iter()
            .map(|v| v.as_str().expect("guard names are strings"))
            .collect();
        assert_eq!(CYCLE_WRITE_READ_ONLY_FIELDS, write_guards.as_slice());
        let issue_guards: Vec<&str> = parsed["cycle_issue_serializer"]["read_only_fields"]
            .as_array()
            .expect("golden carries issue guards")
            .iter()
            .map(|v| v.as_str().expect("guard names are strings"))
            .collect();
        assert_eq!(CYCLE_ISSUE_READ_ONLY_FIELDS, issue_guards.as_slice());
        let props_guards: Vec<&str> = parsed["cycle_user_properties_serializer"]
            ["read_only_fields"]
            .as_array()
            .expect("golden carries user-properties guards")
            .iter()
            .map(|v| v.as_str().expect("guard names are strings"))
            .collect();
        assert_eq!(
            CYCLE_USER_PROPERTIES_READ_ONLY_FIELDS,
            props_guards.as_slice()
        );
    }

    #[test]
    fn golden_error_string_matches_const() {
        let golden = std::fs::read_to_string(FIXTURE_GOLDEN).expect("F-C27-01 golden exists");
        let parsed: serde_json::Value =
            serde_json::from_str(&golden).expect("F-C27-01 golden is valid JSON");
        let golden_error = parsed["cycle_write_serializer"]["cases"][1]["output"]
            ["non_field_errors"][0]
            .as_str()
            .expect("golden carries the ordering error");
        assert_eq!(golden_error, START_AFTER_END_MESSAGE);
    }

    #[test]
    fn start_after_end_rejected_with_byte_identical_body() {
        assert_eq!(
            validate_date_order(Some(10), Some(5)),
            Err(START_AFTER_END_MESSAGE)
        );
        assert_eq!(
            date_order_error_body(),
            r#"{"non_field_errors":["Start date cannot exceed end date"]}"#
        );
    }

    #[test]
    fn null_or_lone_dates_skip_error_and_rewrite() {
        // cycle.py:16-23 — both guards require non-None on both sides.
        assert_eq!(validate_date_order(None, Some(5)), Ok(()));
        assert_eq!(validate_date_order(Some(10), None), Ok(()));
        assert_eq!(validate_date_order(None, None), Ok(()));
        assert_eq!(validate_date_order(Some(5), Some(10)), Ok(()));
        assert_eq!(validate_date_order(Some(5), Some(5)), Ok(()));
    }

    #[test]
    fn project_id_resolution_first_non_null_wins() {
        assert_eq!(
            resolve_rewrite_project_id(Some(1), Some(2), Some(3)),
            Some(1)
        );
        assert_eq!(resolve_rewrite_project_id(None, Some(2), Some(3)), Some(2));
        assert_eq!(resolve_rewrite_project_id(None, None, Some(3)), Some(3));
        assert_eq!(resolve_rewrite_project_id(None, None, None), None);
    }

    #[test]
    fn convert_boundaries_mirror_timezone_converter() {
        // Project at +05:30 (e.g. Asia/Kolkata, no DST): date 2026-03-01.
        // Local midnight as UTC epoch: 2026-03-01T00:00:00+05:30.
        let offset = 5 * 3600 + 30 * 60;
        let day_epoch_utc: i64 = 1_772_323_200; // 2026-03-01T00:00:00Z
        let local_midnight = day_epoch_utc - offset;
        // Start: +1s over local midnight, shifted to UTC.
        assert_eq!(
            convert_start_to_utc(local_midnight, false, 0),
            local_midnight + 1
        );
        assert_eq!(
            render_drf_datetime_utc(convert_start_to_utc(local_midnight, false, 0)),
            "2026-02-28T18:30:01Z"
        );
        // End: local 23:59:00 shifted to UTC; no same-day branch.
        assert_eq!(
            render_drf_datetime_utc(convert_end_to_utc(local_midnight)),
            "2026-03-01T18:29:00Z"
        );
        // Same-day start returns now-in-UTC (timezone_converter.py:82-83).
        assert_eq!(
            convert_start_to_utc(local_midnight, true, 9_999_999_999),
            9_999_999_999
        );
    }

    #[test]
    fn rewrite_guard_requires_date_and_timezone() {
        assert!(rewrite_inputs_present(
            Some("2026-03-01"),
            Some("Asia/Kolkata")
        ));
        assert!(!rewrite_inputs_present(None, Some("Asia/Kolkata")));
        assert!(!rewrite_inputs_present(Some("2026-03-01"), None));
        assert!(!rewrite_inputs_present(Some(""), Some("Asia/Kolkata")));
    }

    #[test]
    fn drf_datetime_renders_z_for_utc() {
        assert_eq!(render_drf_datetime_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            render_drf_datetime_utc(1_772_323_201),
            "2026-03-01T00:00:01Z"
        );
        assert_eq!(
            render_drf_datetime_with_offset(1_772_323_201, 0),
            "2026-03-01T00:00:01Z"
        );
        assert_eq!(
            render_drf_datetime_with_offset(1_772_323_201, 19_800),
            "2026-03-01T05:30:01+05:30"
        );
    }

    #[test]
    fn nested_issue_detail_shape_consts() {
        assert_eq!(CYCLE_ISSUE_NESTED_FIELD, "issue_detail");
        assert_eq!(CYCLE_ISSUE_NESTED_SOURCE, "issue");
        assert_eq!(CYCLE_ISSUE_COUNT_FIELD, "sub_issues_count");
    }
}
